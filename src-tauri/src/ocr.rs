use std::io::Cursor;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::Emitter;
use tauri_plugin_clipboard_manager::ClipboardExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn runtime_log(message: impl AsRef<str>) {
    let base = std::env::var("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let dir = base.join("Mighty-Productivity-OS-Windows");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("mightyvoice-runtime.log");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = format!("[{}] {}\n", ts, message.as_ref());
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrSettings {
    pub ocr_hotkey: String,
    pub ocr_language: String,
    pub translate_enabled: bool,
    pub translate_target: String,
    pub copy_mode: String,    // original | translated | both
    pub image_format: String, // png | jpeg
    pub jpeg_quality: u8,     // 1..100
    pub speed_mode: String,   // local | fast | balanced | accuracy
}

impl Default for OcrSettings {
    fn default() -> Self {
        Self {
            ocr_hotkey: "CmdOrCtrl+Shift+2".to_string(),
            ocr_language: "auto".to_string(),
            translate_enabled: false,
            translate_target: "en".to_string(),
            copy_mode: "original".to_string(),
            image_format: "png".to_string(),
            jpeg_quality: 90,
            speed_mode: "local".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weak_ocr_detection_flags_empty_or_tiny_output() {
        assert!(is_weak_ocr_text(""));
        assert!(is_weak_ocr_text("  ok  "));
        assert!(!is_weak_ocr_text("This is enough recognized text."));
    }

    #[test]
    fn retry_enhancement_returns_valid_png_with_larger_or_equal_dimensions() {
        let img =
            DynamicImage::ImageRgba8(ImageBuffer::from_pixel(80, 30, Rgba([160, 160, 160, 255])));
        let mut input = Cursor::new(Vec::<u8>::new());
        img.write_to(&mut input, ImageFormat::Png).unwrap();

        let enhanced = enhance_image_for_retry(input.into_inner()).unwrap();
        let decoded = image::load_from_memory(&enhanced).unwrap();

        assert!(decoded.width() >= 80);
        assert!(decoded.height() >= 30);
    }

    #[test]
    fn ocr_path_label_matches_status_text() {
        assert_eq!(ocr_path_label("local"), "Local OCR");
        assert_eq!(ocr_path_label("cloud"), "Cloud OCR");
        assert_eq!(ocr_path_label("cloud_fallback"), "Cloud Fallback");
    }
}

fn is_weak_ocr_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return true;
    }
    let useful_chars = trimmed.chars().filter(|c| c.is_alphanumeric()).count();
    useful_chars < 4
}

fn ocr_path_label(path: &str) -> &'static str {
    match path {
        "local" => "Local OCR",
        "cloud_fallback" => "Cloud Fallback",
        _ => "Cloud OCR",
    }
}

fn emit_ocr_status(app: &tauri::AppHandle, path: &str, elapsed_ms: u128) {
    let label = ocr_path_label(path);
    let _ = app.emit(
        "ocr-status",
        json!({
            "path": path,
            "label": label,
            "elapsedMs": elapsed_ms,
        }),
    );
    let _ = app.emit(
        "app-log",
        format!("✅ OCR copied to clipboard ({} · {}ms)", label, elapsed_ms),
    );
}

fn enhance_image_for_retry(png_bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(&png_bytes)
        .map_err(|e| format!("Failed to decode image for retry: {e}"))?;
    let gray = img.to_luma8();
    let (w, h) = gray.dimensions();
    let mut enhanced = ImageBuffer::new(w, h);

    for (x, y, pixel) in gray.enumerate_pixels() {
        let v = pixel[0] as f32;
        let contrasted = ((v - 128.0) * 1.45 + 128.0).clamp(0.0, 255.0) as u8;
        let sharpened = if contrasted > 150 {
            255
        } else if contrasted < 105 {
            0
        } else {
            contrasted
        };
        enhanced.put_pixel(x, y, image::Luma([sharpened]));
    }

    let dyn_img = DynamicImage::ImageLuma8(enhanced);
    let long = w.max(h);
    let retry_img = if long < 1600 {
        dyn_img.resize_exact(
            (w * 2).max(1),
            (h * 2).max(1),
            image::imageops::FilterType::CatmullRom,
        )
    } else {
        dyn_img
    };

    let mut out = Cursor::new(Vec::<u8>::new());
    retry_img
        .write_to(&mut out, ImageFormat::Png)
        .map_err(|e| format!("Failed to encode retry image: {e}"))?;
    Ok(out.into_inner())
}

fn capture_region_to_clipboard() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        runtime_log("quick OCR capture: clearing clipboard and launching ms-screenclip");
        if let Ok(mut clipboard) = Clipboard::new() {
            let _ = clipboard.set_text("MIGHTY_VOICE_WAITING_FOR_SCREENSHOT");
        }
        std::process::Command::new("explorer.exe")
            .arg("ms-screenclip:")
            .spawn()
            .map_err(|e| format!("Failed to launch Windows snip overlay: {e}"))?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("screencapture")
            .args(["-i", "-c"])
            .status()
            .map_err(|e| format!("Failed to launch macOS screenshot capture: {e}"))?;
        if !status.success() {
            return Err("Screenshot capture was cancelled or failed".to_string());
        }
        return Ok(());
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Err("Quick OCR capture currently supports Windows and macOS.".to_string())
    }
}

fn wait_clipboard_image(timeout_ms: u64) -> Result<Vec<u8>, String> {
    let started = Instant::now();
    let mut clipboard = Clipboard::new().map_err(|e| format!("Clipboard access failed: {e}"))?;

    while started.elapsed() < Duration::from_millis(timeout_ms) {
        if let Ok(img) = clipboard.get_image() {
            runtime_log(format!(
                "clipboard image received: {}x{} bytes={}",
                img.width,
                img.height,
                img.bytes.len()
            ));
            let mut rgba = Vec::with_capacity(img.bytes.len());
            for px in img.bytes.chunks_exact(4) {
                // arboard on Windows often returns BGRA; convert to RGBA
                rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
            }
            let Some(buffer): Option<ImageBuffer<Rgba<u8>, Vec<u8>>> =
                ImageBuffer::from_raw(img.width as u32, img.height as u32, rgba)
            else {
                return Err("Failed to parse screenshot image bytes".to_string());
            };
            let dyn_img = DynamicImage::ImageRgba8(buffer);
            let mut out = Cursor::new(Vec::<u8>::new());
            dyn_img
                .write_to(&mut out, ImageFormat::Png)
                .map_err(|e| format!("Failed to encode screenshot: {e}"))?;
            return Ok(out.into_inner());
        }
        std::thread::sleep(Duration::from_millis(150));
    }

    runtime_log("clipboard image wait timed out");
    Err("Screenshot capture cancelled or timed out. Press Esc to cancel the snip overlay, or drag a region to OCR.".to_string())
}

fn maybe_convert_format(
    png_bytes: Vec<u8>,
    settings: &OcrSettings,
) -> Result<(Vec<u8>, &'static str), String> {
    // Speed optimization: downscale before upload depending on speed mode.
    let base =
        image::load_from_memory(&png_bytes).map_err(|e| format!("Failed to decode image: {e}"))?;
    let (max_edge, force_jpeg, default_quality) = match settings.speed_mode.as_str() {
        "local" => (2200u32, false, 90u8),
        "fast" => (1280u32, true, 65u8),
        "accuracy" => (2200u32, false, 92u8),
        _ => (1700u32, false, 78u8), // balanced
    };

    let resized = {
        let w = base.width();
        let h = base.height();
        let long = w.max(h);
        if long > max_edge {
            let ratio = max_edge as f32 / long as f32;
            let nw = ((w as f32) * ratio).max(1.0) as u32;
            let nh = ((h as f32) * ratio).max(1.0) as u32;
            base.resize_exact(nw, nh, image::imageops::FilterType::Triangle)
        } else {
            base
        }
    };

    let wants_jpeg = force_jpeg
        || settings.image_format.eq_ignore_ascii_case("jpeg")
        || settings.image_format.eq_ignore_ascii_case("jpg");
    if wants_jpeg {
        let mut out = Cursor::new(Vec::<u8>::new());
        let quality = if settings.jpeg_quality == 0 {
            default_quality
        } else {
            settings.jpeg_quality.clamp(1, 100)
        };
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
        encoder
            .encode_image(&resized)
            .map_err(|e| format!("Failed to encode JPEG: {e}"))?;
        Ok((out.into_inner(), "image/jpeg"))
    } else {
        let mut out = Cursor::new(Vec::<u8>::new());
        resized
            .write_to(&mut out, ImageFormat::Png)
            .map_err(|e| format!("Failed to encode PNG: {e}"))?;
        Ok((out.into_inner(), "image/png"))
    }
}

fn b64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= input.len() {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | input[i + 2] as u32;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push(TABLE[(n & 63) as usize] as char);
        i += 3;
    }
    let rem = input.len() - i;
    if rem == 1 {
        let n = (input[i] as u32) << 16;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push('=');
    }
    out
}

#[cfg(target_os = "windows")]
fn local_ocr_windows(image_bytes: &[u8], lang: &str) -> Result<String, String> {
    let mut path = std::env::temp_dir();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    path.push(format!("mightyvoice_ocr_{}.png", ts));
    std::fs::write(&path, image_bytes).map_err(|e| format!("Failed to write temp image: {e}"))?;

    let lang_tag = if lang == "auto" { "" } else { lang };
    let escaped_path = path.to_string_lossy().replace("'", "''");
    let escaped_lang = lang_tag.replace("'", "''");

    let ps = format!(
        "$ErrorActionPreference='Stop'; \
Add-Type -AssemblyName System.Runtime.WindowsRuntime; \
$null=[Windows.Storage.StorageFile,Windows.Storage,ContentType=WindowsRuntime]; \
$null=[Windows.Storage.Streams.IRandomAccessStreamWithContentType,Windows.Storage.Streams,ContentType=WindowsRuntime]; \
$null=[Windows.Media.Ocr.OcrEngine,Windows.Media.Ocr,ContentType=WindowsRuntime]; \
$null=[Windows.Media.Ocr.OcrResult,Windows.Media.Ocr,ContentType=WindowsRuntime]; \
$null=[Windows.Graphics.Imaging.BitmapDecoder,Windows.Graphics.Imaging,ContentType=WindowsRuntime]; \
$null=[Windows.Graphics.Imaging.SoftwareBitmap,Windows.Graphics.Imaging,ContentType=WindowsRuntime]; \
function AwaitTyped($op, [Type]$resultType) {{ \
  $method=[System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {{ $_.Name -eq 'AsTask' -and $_.IsGenericMethodDefinition -and $_.GetGenericArguments().Count -eq 1 -and $_.GetParameters().Count -eq 1 }} | Select-Object -First 1; \
  $task=$method.MakeGenericMethod($resultType).Invoke($null, @($op)); \
  $task.GetAwaiter().GetResult() \
}}; \
$file=AwaitTyped ([Windows.Storage.StorageFile]::GetFileFromPathAsync('{path}')) ([Windows.Storage.StorageFile]); \
$stream=AwaitTyped ($file.OpenReadAsync()) ([Windows.Storage.Streams.IRandomAccessStreamWithContentType]); \
$decoder=AwaitTyped ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream)) ([Windows.Graphics.Imaging.BitmapDecoder]); \
$bmp=AwaitTyped ($decoder.GetSoftwareBitmapAsync()) ([Windows.Graphics.Imaging.SoftwareBitmap]); \
$engine=$null; \
if ('{lang}' -ne '') {{ try {{ $engine=[Windows.Media.Ocr.OcrEngine]::TryCreateFromLanguage((New-Object Windows.Globalization.Language('{lang}'))) }} catch {{}} }}; \
if ($null -eq $engine) {{ $engine=[Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages() }}; \
if ($null -eq $engine) {{ throw 'Windows OCR engine unavailable' }}; \
$result=AwaitTyped ($engine.RecognizeAsync($bmp)) ([Windows.Media.Ocr.OcrResult]); \
Write-Output $result.Text;",
        path = escaped_path,
        lang = escaped_lang
    );

    runtime_log(format!(
        "local OCR PowerShell starting: path={} lang={}",
        path.display(),
        lang
    ));
    let mut command = std::process::Command::new("powershell.exe");
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-WindowStyle",
        "Hidden",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        &ps,
    ]);
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);
    let output = command
        .output()
        .map_err(|e| format!("Failed to run PowerShell OCR: {e}"))?;

    let _ = std::fs::remove_file(&path);

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        runtime_log(format!("local OCR PowerShell failed: {}", err));
        return Err(if err.is_empty() {
            "Local OCR command failed".to_string()
        } else {
            err
        });
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    runtime_log(format!("local OCR returned chars={}", text.chars().count()));
    if text.is_empty() {
        return Err("Local OCR returned empty text".to_string());
    }
    Ok(text)
}

fn api_provider(api_key: &str) -> &'static str {
    if api_key.starts_with("gsk_") {
        "groq"
    } else if api_key.starts_with("xai-") {
        "xai"
    } else {
        "none"
    }
}

fn has_supported_cloud_key(api_key: &str) -> bool {
    matches!(api_provider(api_key), "groq" | "xai")
}

fn chat_endpoint(api_key: &str) -> &'static str {
    if api_key.starts_with("xai-") {
        "https://api.x.ai/v1/chat/completions"
    } else {
        "https://api.groq.com/openai/v1/chat/completions"
    }
}

fn vision_model(api_key: &str) -> String {
    if api_key.starts_with("xai-") {
        std::env::var("XAI_VISION_MODEL").unwrap_or_else(|_| "grok-2-vision-1212".to_string())
    } else {
        std::env::var("GROQ_VISION_MODEL")
            .unwrap_or_else(|_| "meta-llama/llama-4-scout-17b-16e-instruct".to_string())
    }
}

fn text_model(api_key: &str) -> String {
    if api_key.starts_with("xai-") {
        std::env::var("XAI_TEXT_MODEL").unwrap_or_else(|_| "grok-3-mini".to_string())
    } else {
        "llama-3.3-70b-versatile".to_string()
    }
}

async fn call_vision_ocr(
    api_key: &str,
    image_bytes: &[u8],
    mime: &str,
    lang: &str,
) -> Result<String, String> {
    let client = Client::new();
    let image_data_url = format!("data:{};base64,{}", mime, b64(image_bytes));
    let lang_hint = if lang == "auto" { "auto-detect" } else { lang };

    let payload = json!({
        "model": vision_model(api_key),
        "messages": [
            {
                "role": "system",
                "content": "You are OCR-only. Extract visible text faithfully. No explanations. Keep line breaks where sensible."
            },
            {
                "role": "user",
                "content": [
                    {"type":"text","text": format!("OCR this screenshot. Language hint: {}. Return only extracted text.", lang_hint)},
                    {"type":"image_url","image_url":{"url": image_data_url}}
                ]
            }
        ],
        "temperature": 0.0
    });

    let res = client
        .post(chat_endpoint(api_key))
        .bearer_auth(api_key)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("OCR request failed: {e}"))?;

    let v: serde_json::Value = res
        .json()
        .await
        .map_err(|e| format!("OCR parse failed: {e}"))?;
    let text = v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() {
        return Err(format!("Cloud OCR failed or returned no text. Keep OCR Speed on Local, or check your provider vision model setting. Response: {}", v));
    }
    Ok(text)
}

async fn translate_text(
    api_key: &str,
    original: &str,
    target_lang: &str,
) -> Result<String, String> {
    let client = Client::new();
    let payload = json!({
        "model": text_model(api_key),
        "messages": [
            {
                "role": "system",
                "content": "Translate the user text to the target language. Output only translated text."
            },
            {
                "role": "user",
                "content": format!("Target language: {}\n\nText:\n{}", target_lang, original)
            }
        ],
        "temperature": 0.0
    });

    let res = client
        .post(chat_endpoint(api_key))
        .bearer_auth(api_key)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Translate request failed: {e}"))?;

    let v: serde_json::Value = res
        .json()
        .await
        .map_err(|e| format!("Translate parse failed: {e}"))?;
    Ok(v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string())
}

pub async fn run_ocr_image_bytes(
    app: tauri::AppHandle,
    png_bytes: Vec<u8>,
    settings: OcrSettings,
    api_key: String,
) -> Result<String, String> {
    runtime_log(format!(
        "OCR image bytes start: input_bytes={} speed={} format={} copy_mode={}",
        png_bytes.len(),
        settings.speed_mode,
        settings.image_format,
        settings.copy_mode
    ));
    let api_key = api_key.trim().to_string();
    let has_cloud_key = has_supported_cloud_key(&api_key);
    if !api_key.is_empty() && !has_cloud_key {
        runtime_log("OCR cloud fallback disabled: saved key provider is unsupported");
    }
    if !has_cloud_key && settings.speed_mode != "local" {
        runtime_log("OCR blocked: missing supported cloud API key and speed mode is not local");
        return Err("Cloud API key is missing or invalid. Add a Groq key starting with gsk_, add an xAI key starting with xai-, or switch OCR Speed to Local.".to_string());
    }

    let started = Instant::now();
    let (encoded, mime) = maybe_convert_format(png_bytes, &settings)?;
    let _ = app.emit(
        "app-log",
        format!(
            "🧠 Running OCR ({}, speed: {})...",
            settings.image_format, settings.speed_mode
        ),
    );

    let mut ocr_path = "cloud";
    let original = if settings.speed_mode == "local" {
        #[cfg(target_os = "windows")]
        {
            let bytes = encoded.clone();
            let lang = settings.ocr_language.clone();
            match tauri::async_runtime::spawn_blocking(move || local_ocr_windows(&bytes, &lang))
                .await
            {
                Ok(Ok(text)) if !is_weak_ocr_text(&text) => {
                    ocr_path = "local";
                    text
                }
                Ok(Ok(_text)) => {
                    let _ = app.emit(
                        "app-log",
                        "⚠️ Local OCR was weak; retrying once with enhanced image...",
                    );
                    match enhance_image_for_retry(encoded.clone()) {
                        Ok(retry_bytes) => {
                            let retry_lang = settings.ocr_language.clone();
                            let retry_for_local = retry_bytes.clone();
                            match tauri::async_runtime::spawn_blocking(move || {
                                local_ocr_windows(&retry_for_local, &retry_lang)
                            })
                            .await
                            {
                                Ok(Ok(retry_text)) if !is_weak_ocr_text(&retry_text) => {
                                    ocr_path = "local";
                                    retry_text
                                }
                                Ok(Ok(_)) | Ok(Err(_)) | Err(_) => {
                                    let _ = app.emit(
                                        "app-log",
                                        "⚠️ Local retry still weak, falling back to cloud OCR...",
                                    );
                                    ocr_path = "cloud_fallback";
                                    if !has_cloud_key {
                                        return Err("Local OCR returned no text, and no supported cloud fallback key is saved. Add a Groq gsk_ key, an xAI xai- key, or try a clearer/larger text region.".to_string());
                                    }
                                    call_vision_ocr(
                                        &api_key,
                                        &retry_bytes,
                                        "image/png",
                                        &settings.ocr_language,
                                    )
                                    .await?
                                }
                            }
                        }
                        Err(err) => {
                            let _ = app.emit(
                                "app-log",
                                format!(
                                    "⚠️ Retry preprocessing failed, falling back to cloud: {}",
                                    err
                                ),
                            );
                            ocr_path = "cloud_fallback";
                            if !has_cloud_key {
                                return Err("Local OCR preprocessing failed, and no supported cloud fallback key is saved. Add a Groq gsk_ key, an xAI xai- key, or try a clearer/larger text region.".to_string());
                            }
                            call_vision_ocr(&api_key, &encoded, mime, &settings.ocr_language)
                                .await?
                        }
                    }
                }
                Ok(Err(err)) => {
                    let _ = app.emit(
                        "app-log",
                        format!("⚠️ Local OCR failed, falling back to cloud: {}", err),
                    );
                    ocr_path = "cloud_fallback";
                    if !has_cloud_key {
                        return Err("Local OCR failed, and no supported cloud fallback key is saved. Add a Groq gsk_ key, an xAI xai- key, or try a clearer/larger text region.".to_string());
                    }
                    call_vision_ocr(&api_key, &encoded, mime, &settings.ocr_language).await?
                }
                Err(err) => {
                    let _ = app.emit(
                        "app-log",
                        format!("⚠️ Local OCR thread failed, falling back to cloud: {}", err),
                    );
                    ocr_path = "cloud_fallback";
                    if !has_cloud_key {
                        return Err("Local OCR failed, and no supported cloud fallback key is saved. Add a Groq gsk_ key, an xAI xai- key, or try a clearer/larger text region.".to_string());
                    }
                    call_vision_ocr(&api_key, &encoded, mime, &settings.ocr_language).await?
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            if !has_cloud_key {
                return Err(
                    "Cloud API key is missing or invalid. Add a Groq gsk_ key or an xAI xai- key."
                        .to_string(),
                );
            }
            call_vision_ocr(&api_key, &encoded, mime, &settings.ocr_language).await?
        }
    } else {
        if !has_cloud_key {
            return Err(
                "Cloud API key is missing or invalid. Add a Groq gsk_ key or an xAI xai- key."
                    .to_string(),
            );
        }
        call_vision_ocr(&api_key, &encoded, mime, &settings.ocr_language).await?
    };

    let translated = if settings.translate_enabled {
        let _ = app.emit(
            "app-log",
            format!("🌐 Translating to {}...", settings.translate_target),
        );
        Some(translate_text(&api_key, &original, &settings.translate_target).await?)
    } else {
        None
    };

    let final_text = match settings.copy_mode.as_str() {
        "translated" => translated.clone().unwrap_or_else(|| original.clone()),
        "both" => {
            if let Some(t) = translated.clone() {
                format!("{}\n\n---\n\n{}", original, t)
            } else {
                original.clone()
            }
        }
        _ => original.clone(),
    };

    app.clipboard()
        .write_text(final_text.clone())
        .map_err(|e| format!("Failed to write clipboard: {e}"))?;
    runtime_log(format!(
        "OCR text copied to clipboard: chars={}",
        final_text.chars().count()
    ));

    let _ = app.emit("ocr-result", final_text.clone());
    emit_ocr_status(&app, ocr_path, started.elapsed().as_millis());

    Ok(final_text)
}

pub async fn run_quick_ocr_capture(
    app: tauri::AppHandle,
    settings: OcrSettings,
    api_key: String,
) -> Result<String, String> {
    let _ = app.emit(
        "ocr-status",
        json!({"path":"capture", "label":"Selecting region", "elapsedMs":0}),
    );
    let _ = app.emit(
        "app-log",
        "📸 Quick OCR: drag a screen region. Press Esc to cancel the snip overlay.",
    );
    capture_region_to_clipboard()?;

    let png_bytes = tauri::async_runtime::spawn_blocking(move || wait_clipboard_image(30_000))
        .await
        .map_err(|e| format!("Capture thread failed: {e}"))??;

    run_ocr_image_bytes(app, png_bytes, settings, api_key).await
}
