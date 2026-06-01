use tauri::{Emitter, Manager};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState, Shortcut};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use std::sync::Arc;
use std::sync::Mutex;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Dwm::DwmFlush;
use crate::ocr::OcrSettings;
use serde::{Deserialize, Serialize};
use ab_glyph::{FontArc, PxScale};
use imageproc::drawing::draw_text_mut;

struct AppState {
    recording_mode: Mutex<String>,
    language: Mutex<String>,
    api_key: Mutex<String>,
    current_hotkey: Mutex<String>,
    current_ocr_hotkey: Mutex<String>,
    current_screenshot_hotkey: Mutex<String>,
    ocr_settings: Mutex<OcrSettings>,
    ocr_capture_in_progress: Mutex<bool>,
    tx: mpsc::Sender<bool>,
    audio_recorder: Arc<audio::AudioRecorder>,
}

fn persisted_api_key_path() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join("Mighty Voice OS").join("groq_api_key.txt")
}

fn load_persisted_api_key() -> String {
    let from_env = std::env::var("GROQ_API_KEY").unwrap_or_default().trim().to_string();
    if from_env.starts_with("gsk_") {
        return from_env;
    }
    std::fs::read_to_string(persisted_api_key_path())
        .unwrap_or_default()
        .trim()
        .to_string()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiKeyStatus {
    has_key: bool,
    source: String,
    path: String,
}

#[tauri::command]
fn get_api_key_status(state: tauri::State<AppState>) -> ApiKeyStatus {
    let from_env = std::env::var("GROQ_API_KEY").unwrap_or_default().trim().to_string();
    let path = persisted_api_key_path();
    let from_file = std::fs::read_to_string(&path).unwrap_or_default().trim().to_string();
    let current = state.api_key.lock().map(|k| k.clone()).unwrap_or_default();
    let has_env = from_env.starts_with("gsk_");
    let has_file = from_file.starts_with("gsk_");
    let has_current = current.trim().starts_with("gsk_");
    let source = if has_env {
        "environment"
    } else if has_file {
        "settings_file"
    } else if has_current {
        "current_session"
    } else {
        "none"
    };
    ApiKeyStatus {
        has_key: has_env || has_file || has_current,
        source: source.to_string(),
        path: path.to_string_lossy().to_string(),
    }
}

#[tauri::command]
fn set_app_state(state: tauri::State<AppState>, recording_mode: String, language: String) {
    if let Ok(mut mode) = state.recording_mode.lock() { *mode = recording_mode; }
    if let Ok(mut lang) = state.language.lock() { *lang = language; }
}

#[tauri::command]
fn set_api_key(state: tauri::State<AppState>, api_key: String) -> Result<(), String> {
    let api_key = api_key.trim().to_string();
    if !api_key.is_empty() && !api_key.starts_with("gsk_") {
        crate::ocr::runtime_log("Groq API key rejected: incompatible prefix");
        return Err("Mighty Voice currently uses Groq for voice transcription. Save a Groq key starting with gsk_, or clear the field. xAI keys cannot be sent to Groq.".to_string());
    }

    if let Ok(mut key) = state.api_key.lock() {
        *key = api_key.clone();
    }

    let path = persisted_api_key_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if api_key.is_empty() {
        let _ = std::fs::remove_file(&path);
        crate::ocr::runtime_log("Groq API key cleared");
    } else {
        std::fs::write(&path, api_key).map_err(|e| format!("Failed to persist Groq API key: {e}"))?;
        crate::ocr::runtime_log("Groq API key saved: valid Groq key present");
    }
    Ok(())
}

#[tauri::command]
fn list_input_devices() -> Vec<String> {
    audio::list_input_devices()
}

#[tauri::command]
fn set_input_device(state: tauri::State<AppState>, device_name: String) {
    let name = if device_name.is_empty() { None } else { Some(device_name) };
    state.audio_recorder.set_preferred_device(name);
}


#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScreenshotRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

fn ps_quote(value: &str) -> String {
    value.replace('\'', "''")
}


#[derive(Debug, Deserialize, Clone, Copy)]
struct AnnotationPoint { x: f64, y: f64 }

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ScreenshotAnnotation {
    #[serde(rename = "pen")]
    Pen { points: Vec<AnnotationPoint> },
    #[serde(rename = "rect")]
    Rect { start: AnnotationPoint, end: AnnotationPoint },
    #[serde(rename = "arrow")]
    Arrow { start: AnnotationPoint, end: AnnotationPoint },
    #[serde(rename = "text")]
    Text { point: AnnotationPoint, text: String },
    #[serde(rename = "blur")]
    Blur { start: AnnotationPoint, end: AnnotationPoint },
    #[serde(rename = "pixelate")]
    Pixelate { start: AnnotationPoint, end: AnnotationPoint },
}

fn draw_line_rgba(img: &mut image::RgbaImage, a: AnnotationPoint, b: AnnotationPoint, color: image::Rgba<u8>) {
    let (mut x0, mut y0) = (a.x.round() as i32, a.y.round() as i32);
    let (x1, y1) = (b.x.round() as i32, b.y.round() as i32);
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        for ox in -1..=1 { for oy in -1..=1 {
            let x = x0 + ox; let y = y0 + oy;
            if x >= 0 && y >= 0 && (x as u32) < img.width() && (y as u32) < img.height() {
                img.put_pixel(x as u32, y as u32, color);
            }
        }}
        if x0 == x1 && y0 == y1 { break; }
        let e2 = 2 * err;
        if e2 >= dy { err += dy; x0 += sx; }
        if e2 <= dx { err += dx; y0 += sy; }
    }
}

fn normalize_pixel_rect(start: AnnotationPoint, end: AnnotationPoint, img_w: u32, img_h: u32) -> Option<(u32, u32, u32, u32)> {
    let x0 = start.x.min(end.x).floor().max(0.0) as i32;
    let y0 = start.y.min(end.y).floor().max(0.0) as i32;
    let x1 = start.x.max(end.x).ceil().min(img_w as f64) as i32;
    let y1 = start.y.max(end.y).ceil().min(img_h as f64) as i32;
    if x1 - x0 < 2 || y1 - y0 < 2 { return None; }
    Some((x0 as u32, y0 as u32, x1 as u32, y1 as u32))
}

fn apply_blur_rect(img: &mut image::RgbaImage, start: AnnotationPoint, end: AnnotationPoint) {
    let (w, h) = (img.width(), img.height());
    let Some((x0, y0, x1, y1)) = normalize_pixel_rect(start, end, w, h) else { return; };
    let src = img.clone();
    let radius: i32 = 6;
    for y in y0..y1 {
        for x in x0..x1 {
            let mut r: u32 = 0;
            let mut g: u32 = 0;
            let mut b: u32 = 0;
            let mut a: u32 = 0;
            let mut c: u32 = 0;
            let xi = x as i32;
            let yi = y as i32;
            for oy in -radius..=radius {
                for ox in -radius..=radius {
                    let nx = (xi + ox).clamp(0, (w - 1) as i32) as u32;
                    let ny = (yi + oy).clamp(0, (h - 1) as i32) as u32;
                    let p = src.get_pixel(nx, ny).0;
                    r += p[0] as u32;
                    g += p[1] as u32;
                    b += p[2] as u32;
                    a += p[3] as u32;
                    c += 1;
                }
            }
            img.put_pixel(x, y, image::Rgba([(r / c) as u8, (g / c) as u8, (b / c) as u8, (a / c) as u8]));
        }
    }
}

fn apply_pixelate_rect(img: &mut image::RgbaImage, start: AnnotationPoint, end: AnnotationPoint) {
    let (w, h) = (img.width(), img.height());
    let Some((x0, y0, x1, y1)) = normalize_pixel_rect(start, end, w, h) else { return; };
    let src = img.clone();
    let block: u32 = 12;
    let mut by = y0;
    while by < y1 {
        let mut bx = x0;
        while bx < x1 {
            let ex = (bx + block).min(x1);
            let ey = (by + block).min(y1);
            let mut r: u32 = 0;
            let mut g: u32 = 0;
            let mut b: u32 = 0;
            let mut a: u32 = 0;
            let mut c: u32 = 0;
            for y in by..ey {
                for x in bx..ex {
                    let p = src.get_pixel(x, y).0;
                    r += p[0] as u32;
                    g += p[1] as u32;
                    b += p[2] as u32;
                    a += p[3] as u32;
                    c += 1;
                }
            }
            if c > 0 {
                let avg = image::Rgba([(r / c) as u8, (g / c) as u8, (b / c) as u8, (a / c) as u8]);
                for y in by..ey {
                    for x in bx..ex {
                        img.put_pixel(x, y, avg);
                    }
                }
            }
            bx += block;
        }
        by += block;
    }
}

fn load_annotation_font() -> Option<FontArc> {
    let candidates = [
        "C:/Windows/Fonts/segoeui.ttf",
        "C:/Windows/Fonts/arial.ttf",
        "C:/Windows/Fonts/calibri.ttf",
    ];
    for p in candidates {
        if let Ok(bytes) = std::fs::read(p) {
            if let Ok(font) = FontArc::try_from_vec(bytes) {
                return Some(font);
            }
        }
    }
    None
}

fn apply_screenshot_annotations(path: &std::path::Path, annotations: &[ScreenshotAnnotation]) -> Result<(), String> {
    if annotations.is_empty() { return Ok(()); }
    let mut img = image::open(path)
        .map_err(|e| format!("Failed to load screenshot for annotations: {e}"))?
        .to_rgba8();
    let green = image::Rgba([34, 197, 94, 255]);
    let text_font = load_annotation_font();
    for ann in annotations {
        match ann {
            ScreenshotAnnotation::Pen { points } => {
                for pair in points.windows(2) { draw_line_rgba(&mut img, pair[0], pair[1], green); }
            }
            ScreenshotAnnotation::Rect { start, end } => {
                let p1 = *start;
                let p2 = AnnotationPoint { x: end.x, y: start.y };
                let p3 = *end;
                let p4 = AnnotationPoint { x: start.x, y: end.y };
                draw_line_rgba(&mut img, p1, p2, green); draw_line_rgba(&mut img, p2, p3, green);
                draw_line_rgba(&mut img, p3, p4, green); draw_line_rgba(&mut img, p4, p1, green);
            }
            ScreenshotAnnotation::Arrow { start, end } => {
                draw_line_rgba(&mut img, *start, *end, green);
                let angle = (end.y - start.y).atan2(end.x - start.x);
                let len = 14.0;
                for delta in [2.6_f64, -2.6_f64] {
                    let p = AnnotationPoint { x: end.x - len * (angle + delta).cos(), y: end.y - len * (angle + delta).sin() };
                    draw_line_rgba(&mut img, *end, p, green);
                }
            }
            ScreenshotAnnotation::Text { point, text } => {
                if !text.trim().is_empty() {
                    if let Some(font) = &text_font {
                        draw_text_mut(
                            &mut img,
                            green,
                            point.x.round() as i32,
                            point.y.round() as i32,
                            PxScale { x: 24.0, y: 24.0 },
                            font,
                            text,
                        );
                    } else {
                        let end = AnnotationPoint { x: point.x + (text.len().max(1) as f64 * 8.0), y: point.y };
                        draw_line_rgba(&mut img, *point, end, green);
                    }
                }
            }
            ScreenshotAnnotation::Blur { start, end } => apply_blur_rect(&mut img, *start, *end),
            ScreenshotAnnotation::Pixelate { start, end } => apply_pixelate_rect(&mut img, *start, *end),
        }
    }
    img.save(path).map_err(|e| format!("Failed to save annotated screenshot: {e}"))?;
    Ok(())
}

fn capture_screen_region_to_png(rect: &ScreenshotRect, output_path: &std::path::Path, copy_to_clipboard: bool) -> Result<(), String> {
    crate::ocr::runtime_log(format!("capture_screen_region_to_png rect x={} y={} w={} h={} copy={}", rect.x, rect.y, rect.width, rect.height, copy_to_clipboard));

    // Flush the DWM compositor so the desktop is fully repainted before we
    // capture. Without this, hiding the always-on-top overlay 180ms ago can
    // leave a stale frame where the underlying window z-order is wrong.
    #[cfg(target_os = "windows")]
    { let _ = unsafe { DwmFlush() }; }
    if rect.width < 8.0 || rect.height < 8.0 {
        crate::ocr::runtime_log("capture_screen_region_to_png rejected: selection too small");
        return Err("Selection too small — drag a larger area.".to_string());
    }
    let path = ps_quote(&output_path.to_string_lossy());
    let set_clipboard = if copy_to_clipboard { "$clip.SetImage($bmp);" } else { "" };
    let ps = format!(
        "$ErrorActionPreference='Stop'; \
Add-Type -AssemblyName System.Windows.Forms; \
Add-Type -AssemblyName System.Drawing; \
$x=[int]{x}; $y=[int]{y}; $w=[int]{w}; $h=[int]{h}; \
$bmp=New-Object System.Drawing.Bitmap $w,$h; \
$g=[System.Drawing.Graphics]::FromImage($bmp); \
$g.CopyFromScreen($x,$y,0,0,$bmp.Size); \
$g.Dispose(); \
$bmp.Save('{path}', [System.Drawing.Imaging.ImageFormat]::Png); \
$clip=[System.Windows.Forms.Clipboard]; {set_clipboard} \
$bmp.Dispose();",
        x = rect.x.round() as i32,
        y = rect.y.round() as i32,
        w = rect.width.round().max(8.0) as i32,
        h = rect.height.round().max(8.0) as i32,
        path = path,
        set_clipboard = set_clipboard,
    );
    let mut command = std::process::Command::new("powershell.exe");
    command.args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-ExecutionPolicy", "Bypass", "-Command", &ps]);
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);
    let output = command
        .output()
        .map_err(|e| format!("Failed to run screenshot capture: {e}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        crate::ocr::runtime_log(format!("screenshot capture PowerShell failed: {}", err));
        return Err(if err.is_empty() { "Screenshot capture failed".to_string() } else { err });
    }
    crate::ocr::runtime_log(format!("screenshot capture PowerShell ok: {}", output_path.display()));
    Ok(())
}


fn capture_png_file_to_clipboard(path: &std::path::Path) -> Result<(), String> {
    let path = ps_quote(&path.to_string_lossy());
    let ps = format!(
        "$ErrorActionPreference='Stop'; Add-Type -AssemblyName System.Windows.Forms; Add-Type -AssemblyName System.Drawing; $bmp=[System.Drawing.Image]::FromFile('{path}'); [System.Windows.Forms.Clipboard]::SetImage($bmp); $bmp.Dispose();",
        path = path,
    );
    let mut command = std::process::Command::new("powershell.exe");
    command.args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-ExecutionPolicy", "Bypass", "-Command", &ps]);
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);
    let output = command.output().map_err(|e| format!("Failed to copy image: {e}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        crate::ocr::runtime_log(format!("copy image PowerShell failed: {}", err));
        return Err(if err.is_empty() { "Copy image failed".to_string() } else { err });
    }
    crate::ocr::runtime_log("copy image PowerShell ok");
    Ok(())
}

#[tauri::command]
fn open_screenshot_overlay(app: tauri::AppHandle, mode: Option<String>) -> Result<(), String> {
    open_screenshot_overlay_with_mode(&app, mode.as_deref().unwrap_or("screenshot"))
}

fn open_screenshot_overlay_with_mode(app: &tauri::AppHandle, mode: &str) -> Result<(), String> {
    let mode = if mode == "ocr" { "ocr" } else { "screenshot" };
    crate::ocr::runtime_log(format!("screenshot overlay open requested mode={}", mode));
    let Some(w) = app.get_webview_window("screenshot_overlay") else {
        return Err("Screenshot overlay window not found".to_string());
    };

    // Make mode delivery robust. The hidden overlay webview may not have registered
    // its JS listener yet when a hotkey opens it, which made Mighty OCR fall back to
    // screenshot mode and require a second toolbar click. Emit before and after show,
    // and repeat shortly after focus so OCR mode is applied reliably.
    let mode_script = format!(
        "window.__MIGHTY_OVERLAY_MODE = '{mode}'; window.dispatchEvent(new CustomEvent('mighty-overlay-mode', {{ detail: '{mode}' }}));"
    );
    let _ = w.eval(&mode_script);
    let _ = app.emit("screenshot-overlay-mode", mode);
    let _ = w.emit("screenshot-overlay-mode", mode);
    let _ = w.set_title(if mode == "ocr" { "Mighty OCR" } else { "Mighty Screenshot" });
    let _ = w.set_fullscreen(true);
    let _ = w.show();
    let _ = w.set_focus();
    let _ = w.eval(&mode_script);
    let _ = app.emit("screenshot-overlay-mode", mode);
    let _ = w.emit("screenshot-overlay-mode", mode);

    let app_for_emit = app.clone();
    let mode_for_emit = mode.to_string();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let _ = app_for_emit.emit("screenshot-overlay-mode", mode_for_emit.as_str());
        if let Some(w) = app_for_emit.get_webview_window("screenshot_overlay") {
            let mode_script = format!(
                "window.__MIGHTY_OVERLAY_MODE = '{mode}'; window.dispatchEvent(new CustomEvent('mighty-overlay-mode', {{ detail: '{mode}' }}));",
                mode = mode_for_emit
            );
            let _ = w.eval(&mode_script);
            let _ = w.emit("screenshot-overlay-mode", mode_for_emit.as_str());
            let _ = w.set_focus();
        }
    });

    Ok(())
}

#[tauri::command]
async fn screenshot_overlay_action(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    action: String,
    rect: ScreenshotRect,
    annotations: Option<serde_json::Value>,
) -> Result<String, String> {
    let mut path = std::env::temp_dir();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    path.push(format!("mightyvoice_screenshot_{}.png", ts));

    // Hide the overlay before capturing so the screenshot contains the user's screen,
    // not Mighty Voice's green selection box. The short delay lets DWM repaint.
    if let Some(w) = app.get_webview_window("screenshot_overlay") {
        let _ = w.hide();
    }
    // Wait for DWM to recompose the desktop. Shorter delays can leave
    // stale z-order where the wrong window appears on top in captures.
    std::thread::sleep(std::time::Duration::from_millis(400));
    let parsed_annotations: Vec<ScreenshotAnnotation> = annotations
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    crate::ocr::runtime_log(format!("screenshot_overlay_action action={} rect={}x{} at {},{}", action, rect.width, rect.height, rect.x, rect.y));
    match action.as_str() {
        "copy" => {
            capture_screen_region_to_png(&rect, &path, false)?;
            apply_screenshot_annotations(&path, &parsed_annotations)?;
            capture_png_file_to_clipboard(&path)?;
            let _ = std::fs::remove_file(&path);
            Ok("Image copied to clipboard".to_string())
        }
        "save" => {
            let mut save_path = dirs::picture_dir().unwrap_or_else(std::env::temp_dir);
            save_path.push(format!("MightyVoice-Screenshot-{}.png", ts));
            capture_screen_region_to_png(&rect, &save_path, false)?;
            apply_screenshot_annotations(&save_path, &parsed_annotations)?;
            Ok(format!("Saved screenshot: {}", save_path.display()))
        }
        "ocr" => {
            capture_screen_region_to_png(&rect, &path, false)?;
            apply_screenshot_annotations(&path, &parsed_annotations)?;
            let bytes = std::fs::read(&path).map_err(|e| format!("Failed to read screenshot: {e}"))?;
            let _ = std::fs::remove_file(&path);
            let settings = state.ocr_settings.lock().unwrap().clone();
            let api_key = state.api_key.lock().unwrap().clone();
            let text = ocr::run_ocr_image_bytes(app.clone(), bytes, settings, api_key).await?;
            Ok(if text.trim().is_empty() { "OCR finished".to_string() } else { "OCR text copied".to_string() })
        }
        _ => Err("Unknown screenshot action".to_string()),
    }
}

#[tauri::command]
fn unregister_hotkey(app: tauri::AppHandle, state: tauri::State<AppState>) -> Result<(), String> {
    let current = state.current_hotkey.lock().unwrap();
    if let Ok(old_sc) = current.parse::<Shortcut>() {
        let _ = app.global_shortcut().unregister(old_sc);
    }
    Ok(())
}

#[tauri::command]
fn update_hotkey(app: tauri::AppHandle, state: tauri::State<AppState>, new_hotkey: String) -> Result<(), String> {
    let mut current = state.current_hotkey.lock().unwrap();
    if *current == new_hotkey { return Ok(()); }

    #[cfg(target_os = "windows")]
    {
        windows_ptt_hook::update_hotkey(&new_hotkey);
        *current = new_hotkey;
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
    if let Ok(old_sc) = current.parse::<Shortcut>() {
        let _ = app.global_shortcut().unregister(old_sc);
    }
    let new_sc: Shortcut = new_hotkey.parse().map_err(|_| "Failed to parse shortcut".to_string())?;
    let tx_clone = state.tx.clone();
    app.global_shortcut().on_shortcut(new_sc, move |_app, _shortcut, event| {
        if event.state() == ShortcutState::Pressed {
            let _ = tx_clone.try_send(true);
        } else if event.state() == ShortcutState::Released {
            let _ = tx_clone.try_send(false);
        }
    }).map_err(|e| format!("Failed to register shortcut: {}", e))?;
    *current = new_hotkey;
    Ok(())
    }
}

#[tauri::command]
fn update_ocr_settings(state: tauri::State<AppState>, settings: OcrSettings) -> Result<(), String> {
    let mut s = state.ocr_settings.lock().unwrap();
    *s = settings;
    Ok(())
}

fn start_quick_ocr_capture(app: tauri::AppHandle) {
    if let Err(err) = open_screenshot_overlay_with_mode(&app, "ocr") {
        let _ = app.emit("app-error", err.clone());
        let _ = app.emit("app-log", format!("❌ OCR overlay error: {}", err));
    }
}

#[tauri::command]
fn unregister_ocr_hotkey(app: tauri::AppHandle, state: tauri::State<AppState>) -> Result<(), String> {
    let current = state.current_ocr_hotkey.lock().unwrap();
    if let Ok(old_sc) = current.parse::<Shortcut>() {
        let _ = app.global_shortcut().unregister(old_sc);
    }
    Ok(())
}

#[tauri::command]
fn update_ocr_hotkey(app: tauri::AppHandle, state: tauri::State<AppState>, new_hotkey: String) -> Result<(), String> {
    let mut current = state.current_ocr_hotkey.lock().unwrap();
    if *current == new_hotkey { return Ok(()); }
    if let Ok(old_sc) = current.parse::<Shortcut>() {
        let _ = app.global_shortcut().unregister(old_sc);
    }

    let new_sc: Shortcut = new_hotkey.parse().map_err(|_| "Failed to parse OCR shortcut".to_string())?;
    let app_handle = app.clone();
    app.global_shortcut().on_shortcut(new_sc, move |_app, _shortcut, event| {
        if event.state() == ShortcutState::Pressed {
            start_quick_ocr_capture(app_handle.clone());
        }
    }).map_err(|e| format!("Failed to register OCR shortcut: {}", e))?;

    *current = new_hotkey;
    Ok(())
}

#[tauri::command]
fn unregister_screenshot_hotkey(app: tauri::AppHandle, state: tauri::State<AppState>) -> Result<(), String> {
    let current = state.current_screenshot_hotkey.lock().unwrap();
    if let Ok(old_sc) = current.parse::<Shortcut>() {
        let _ = app.global_shortcut().unregister(old_sc);
    }
    Ok(())
}

#[tauri::command]
fn update_screenshot_hotkey(app: tauri::AppHandle, state: tauri::State<AppState>, new_hotkey: String) -> Result<(), String> {
    let mut current = state.current_screenshot_hotkey.lock().unwrap();
    if *current == new_hotkey { return Ok(()); }
    if let Ok(old_sc) = current.parse::<Shortcut>() {
        let _ = app.global_shortcut().unregister(old_sc);
    }

    let new_sc: Shortcut = new_hotkey.parse().map_err(|_| "Failed to parse screenshot shortcut".to_string())?;
    let app_handle = app.clone();
    app.global_shortcut().on_shortcut(new_sc, move |_app, _shortcut, event| {
        if event.state() == ShortcutState::Pressed {
            if let Err(err) = open_screenshot_overlay_with_mode(&app_handle, "screenshot") {
                let _ = app_handle.emit("app-error", err.clone());
                let _ = app_handle.emit("app-log", format!("❌ Screenshot overlay error: {}", err));
            }
        }
    }).map_err(|e| format!("Failed to register screenshot shortcut: {}", e))?;

    *current = new_hotkey;
    Ok(())
}

#[tauri::command]
fn run_ocr_capture_now(app: tauri::AppHandle) -> Result<String, String> {
    open_screenshot_overlay_with_mode(&app, "ocr")?;
    Ok("Mighty OCR ready: drag a text region.".to_string())
}

fn simulate_copy_in_pid(pid: &str) {
    // Use osascript to Cmd+C in the specific target app by PID — avoids enigo routing to wrong window
    let script = format!(
        "tell application \"System Events\"\n\
         set frontmost of first application process whose unix id is {pid} to true\n\
         end tell\n\
         delay 0.1\n\
         tell application \"System Events\" to keystroke \"c\" using {{command down}}"
    );
    let _ = std::process::Command::new("osascript")
        .args(["-e", &script])
        .output();
}

fn play_sound(sound: &'static str) {
    std::thread::spawn(move || {
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("afplay")
                .arg(format!("/System/Library/Sounds/{}.aiff", sound))
                .output();
        }
        #[cfg(target_os = "windows")]
        {
            // Map to Windows system sounds via PowerShell
            let ps_type = match sound {
                "Tink" => "Asterisk",   // recording start
                "Ping" => "Asterisk",   // success
                "Basso" | "Funk" => "Hand", // failure
                _ => "Asterisk",
            };
            let _ = std::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden",
                       "-Command", &format!("[System.Media.SystemSounds]::{}.Play()", ps_type)])
                .output();
        }
    });
}

fn show_indicator(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("indicator") {
        let mut target_monitor = None;
        if let Ok(pos) = w.cursor_position() {
            if let Ok(monitors) = w.available_monitors() {
                for m in monitors {
                    let m_pos = m.position();
                    let m_size = m.size();
                    if pos.x >= m_pos.x as f64 && pos.x <= (m_pos.x + m_size.width as i32) as f64 &&
                       pos.y >= m_pos.y as f64 && pos.y <= (m_pos.y + m_size.height as i32) as f64 {
                        target_monitor = Some(m.clone());
                        break;
                    }
                }
            }
        }
        
        if target_monitor.is_none() {
            target_monitor = app.get_webview_window("main").and_then(|mw| mw.primary_monitor().ok().flatten());
        }

        if let Some(monitor) = target_monitor {
            let size = monitor.size();
            let m_pos = monitor.position();
            let scale = monitor.scale_factor();
            let win_w = (200.0 * scale) as i32;
            let win_h = (44.0 * scale) as i32;
            let x = m_pos.x + (size.width as i32 - win_w) / 2;
            let y = m_pos.y + size.height as i32 - win_h - (100.0 * scale) as i32;
            let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
        }
        let _ = w.show();
    }
}

pub mod audio;
pub mod cloud;
pub mod injector;
pub mod ocr;
#[cfg(target_os = "windows")]
pub mod windows_ptt_hook;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    dotenv::dotenv().ok();

    let (tx, mut rx) = mpsc::channel(32);
    let audio_recorder = Arc::new(audio::AudioRecorder::new());

    // Install the push-to-talk low-level keyboard hook immediately, before
    // WebView2 loads. This avoids the several-second gap where the user presses
    // the hotkey and nothing happens because Tauri's setup() hasn't run yet.
    #[cfg(target_os = "windows")]
    windows_ptt_hook::start(tx.clone(), "CmdOrCtrl+Shift+Space");

    tauri::Builder::default()
        .manage(AppState {
            recording_mode: Mutex::new("push_to_talk".to_string()),
            language: Mutex::new("auto".to_string()),
            api_key: Mutex::new(load_persisted_api_key()),
            current_hotkey: Mutex::new("CmdOrCtrl+Shift+Space".to_string()),
            current_ocr_hotkey: Mutex::new("CmdOrCtrl+Shift+2".to_string()),
            current_screenshot_hotkey: Mutex::new("CmdOrCtrl+Shift+3".to_string()),
            ocr_settings: Mutex::new(OcrSettings::default()),
            ocr_capture_in_progress: Mutex::new(false),
            tx: tx.clone(),
            audio_recorder: Arc::clone(&audio_recorder),
        })
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec![])))
        .setup(move |app| {
            let app_handle = app.handle().clone();

            // --- Tray icon ---
            let show_item = MenuItem::with_id(app, "show", "Show Settings", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit Mighty Voice OS", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().expect("no app icon"))
                .tooltip("Mighty Voice OS")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up, ..
                    } = event {
                        let app = tray.app_handle();
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                })
                .build(app)?;

            // --- Hide to tray on window close ---
            let main_win = app.get_webview_window("main").unwrap();
            let main_win_clone = main_win.clone();
            main_win.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = main_win_clone.hide();
                }
            });

            // --- Dictation hotkey ---
            let default_hotkey = "CmdOrCtrl+Shift+Space";

            #[cfg(target_os = "windows")]
            windows_ptt_hook::start(tx.clone(), default_hotkey);

            #[cfg(not(target_os = "windows"))]
            {
            let shortcut: Shortcut = default_hotkey.parse().expect("Failed to parse shortcut");
            let tx_clone = tx.clone();
            if let Err(err) = app.global_shortcut().on_shortcut(shortcut, move |_app, _shortcut, event| {
                if event.state() == ShortcutState::Pressed {
                    let _ = tx_clone.try_send(true);
                } else if event.state() == ShortcutState::Released {
                    let _ = tx_clone.try_send(false);
                }
            }) {
                ocr::runtime_log(format!("startup global dictation shortcut unavailable; continuing without crash: {}", err));
                let _ = app.emit("app-log", format!("⚠️ Dictation hotkey unavailable: {}", err));
            }
            }

            // --- OCR shortcut ---
            let ocr_shortcut: Shortcut = "CmdOrCtrl+Shift+2".parse().expect("Failed to parse OCR shortcut");
            let app_for_ocr = app.handle().clone();
            #[cfg(desktop)]
            if let Err(err) = app.global_shortcut().on_shortcut(ocr_shortcut, move |_app, _shortcut, event| {
                if event.state() == ShortcutState::Pressed {
                    start_quick_ocr_capture(app_for_ocr.clone());
                }
            }) {
                ocr::runtime_log(format!("startup OCR shortcut unavailable; continuing without crash: {}", err));
                let _ = app.emit("app-log", format!("⚠️ OCR hotkey unavailable: {}", err));
            }

            // --- Mighty Screenshot / annotation overlay shortcut ---
            let screenshot_shortcut: Shortcut = "CmdOrCtrl+Shift+3".parse().expect("Failed to parse screenshot shortcut");
            let app_for_screenshot = app.handle().clone();
            #[cfg(desktop)]
            if let Err(err) = app.global_shortcut().on_shortcut(screenshot_shortcut, move |_app, _shortcut, event| {
                if event.state() == ShortcutState::Pressed {
                    if let Err(err) = open_screenshot_overlay_with_mode(&app_for_screenshot, "screenshot") {
                        let _ = app_for_screenshot.emit("app-error", err.clone());
                        let _ = app_for_screenshot.emit("app-log", format!("❌ Screenshot overlay error: {}", err));
                    }
                }
            }) {
                ocr::runtime_log(format!("startup screenshot shortcut unavailable; continuing without crash: {}", err));
                let _ = app.emit("app-log", format!("⚠️ Screenshot hotkey unavailable: {}", err));
            }

            // --- Recording loop ---
            tauri::async_runtime::spawn(async move {
                let our_pid = std::process::id().to_string();
                let mut is_recording = false;
                let mut active_mode = String::from("push_to_talk");
                let mut selected_text: Option<String> = None;
                let mut saved_clipboard: Option<String> = None;
                let mut target_app: Option<String> = None;
                let mut skip_inject = false;
                let mut is_physically_pressed = false;

                while let Some(is_pressed) = rx.recv().await {
                    if is_pressed == is_physically_pressed {
                        continue; // Ignore OS key-repeat
                    }
                    is_physically_pressed = is_pressed;

                    let mode = app_handle.state::<AppState>().recording_mode.lock().unwrap().clone();

                    if is_pressed && !is_recording {
                        ocr::runtime_log(format!("recording loop received start; mode={}", mode));
                        is_recording = true;
                        active_mode = mode.clone();

                        target_app = std::process::Command::new("osascript")
                            .args(["-e", "tell application \"System Events\" to get unix id of first application process whose frontmost is true"])
                            .output()
                            .ok()
                            .and_then(|o| String::from_utf8(o.stdout).ok())
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty());

                        // Don't paste into ourselves
                        skip_inject = target_app.as_deref() == Some(our_pid.as_str());
                        let _ = app_handle.emit("app-log", format!("🎯 Target PID: {:?}{}", target_app, if skip_inject { " (self)" } else { "" }));

                        if active_mode == "command" && !skip_inject {
                            if let Some(pid) = target_app.as_deref() {
                                saved_clipboard = app_handle.clipboard().read_text().ok();
                                sleep(Duration::from_millis(80)).await;
                                let pid_owned = pid.to_string();
                                tauri::async_runtime::spawn_blocking(move || simulate_copy_in_pid(&pid_owned)).await.ok();
                                sleep(Duration::from_millis(200)).await;
                                let new_clipboard = app_handle.clipboard().read_text().ok();
                                selected_text = if new_clipboard != saved_clipboard { new_clipboard } else { None };
                            }
                        } else {
                            selected_text = None;
                            saved_clipboard = None;
                        }

                        show_indicator(&app_handle);
                        let _ = app_handle.emit("recording-state", "started");
                        let _ = app_handle.emit("app-log", format!("🎙 Recording started (mode: {})", active_mode));
                        play_sound("Tink");
                        let _ = audio_recorder.start_recording();

                    } else if is_recording {
                        let should_stop = match active_mode.as_str() {
                            "hands_free" => is_pressed,
                            _ => !is_pressed,
                        };

                        if should_stop {
                            ocr::runtime_log("recording loop received stop; processing audio");
                            is_recording = false;
                            let _ = app_handle.emit("recording-state", "processing");
                            let is_intent_mode = active_mode == "command";
                            let recorder_clone = Arc::clone(&audio_recorder);
                            let app_handle_clone = app_handle.clone();
                            let sel_text = selected_text.clone();
                            let saved_cb = saved_clipboard.clone();
                            let inject_target = target_app.clone();
                            let do_inject = !skip_inject;

                            tauri::async_runtime::spawn(async move {
                                let _ = app_handle_clone.emit("app-log", "⏹ Recording stopped, processing audio...");
                                match recorder_clone.stop_recording_and_get_wav() {
                                    Ok(wav_data) => {
                                        let sample_rate = 44_100u32;
                                        let duration_ms = (wav_data.len() as f32 / 4.0 / sample_rate as f32 * 1000.0) as u32;
                                        let _ = app_handle_clone.emit("app-log", format!("📦 Audio: {} bytes, ~{}ms", wav_data.len(), duration_ms));
                                        if duration_ms < 300 {
                                            let _ = app_handle_clone.emit("app-log", "⚠️ Audio too short (<300ms), skipping");
                                            play_sound("Basso");
                                            if let Some(w) = app_handle_clone.get_webview_window("indicator") { let _ = w.hide(); }
                                            let _ = app_handle_clone.emit("recording-state", "idle");
                                            return;
                                        }
                                        let rms = {
                                            let samples: Vec<f32> = wav_data[44..].chunks(4)
                                                .filter_map(|b| b.try_into().ok().map(f32::from_le_bytes))
                                                .filter(|s| s.is_finite())
                                                .collect();
                                            if samples.is_empty() { 0.0f32 } else {
                                                let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
                                                (sum_sq / samples.len() as f32).sqrt()
                                            }
                                        };
                                        let _ = app_handle_clone.emit("app-log", format!("🔊 RMS: {:.6}", rms));
                                        if rms < 0.003 {
                                            let _ = app_handle_clone.emit("app-log", "⚠️ Audio is silence or mic returned bad data after sleep — try recording again");
                                            play_sound("Basso");
                                            if let Some(w) = app_handle_clone.get_webview_window("indicator") { let _ = w.hide(); }
                                            let _ = app_handle_clone.emit("recording-state", "idle");
                                            return;
                                        }
                                        let current_lang = app_handle_clone.state::<AppState>().language.lock().unwrap().clone();
                                        let api_key = app_handle_clone.state::<AppState>().api_key.lock().unwrap().clone();
                                        let has_groq_key = api_key.trim().starts_with("gsk_");
                                        let _ = app_handle_clone.emit("app-log", if has_groq_key { "🔑 Groq API key: saved" } else { "⚠️ Groq API key missing/invalid (must start with gsk_)" });
                                        if has_groq_key {
                                            let _ = app_handle_clone.emit("app-log", "☁️ Sending to Groq...");
                                        }
                                        ocr::runtime_log(format!("voice processing: wav_bytes={} duration_ms={} rms={:.6} groq_key_valid={}", wav_data.len(), duration_ms, rms, has_groq_key));
                                        match cloud::transcribe_and_refine(wav_data, api_key, is_intent_mode, current_lang, sel_text).await {
                                            Ok(text) => {
                                                let _ = app_handle_clone.emit("app-log", format!("✅ Result: {}", text));
                                                let _ = app_handle_clone.emit("dictation-result", text.clone());
                                                if !text.is_empty() && do_inject {
                                                    let _ = app_handle_clone.emit("app-log", format!("⌨️ Injecting into PID {:?}...", inject_target));
                                                    play_sound("Ping");
                                                    injector::inject_via_clipboard(&app_handle_clone, &text, inject_target.as_deref());
                                                } else if !text.is_empty() {
                                                    let _ = app_handle_clone.emit("app-log", "📋 Result ready (no inject — self was focused)");
                                                    play_sound("Ping");
                                                }
                                            }
                                            Err(err) => {
                                                eprintln!("Transcription failed: {}", err);
                                                ocr::runtime_log(format!("voice transcription failed: {}", err));
                                                let _ = app_handle_clone.emit("app-log", format!("❌ Error: {}", err));
                                                let _ = app_handle_clone.emit("app-error", err);
                                                play_sound("Basso");
                                            }
                                        }
                                    }
                                    Err(err) => {
                                        eprintln!("Audio capture failed: {:?}", err);
                                        let _ = app_handle_clone.emit("app-log", format!("❌ Audio error: {}", err));
                                        let _ = app_handle_clone.emit("app-error", format!("Audio capture failed: {err}"));
                                        play_sound("Basso");
                                    }
                                }

                                if let Some(cb) = saved_cb {
                                    let _ = app_handle_clone.clipboard().write_text(cb);
                                }
                                if let Some(w) = app_handle_clone.get_webview_window("indicator") { let _ = w.hide(); }
                                let _ = app_handle_clone.emit("recording-state", "idle");
                            });
                        }
                    }
                }
            });

            // --- Startup complete ---
            play_sound("Tink");
            ocr::runtime_log("startup complete — hotkeys active");
            let _ = app_handle.emit("app-log", "✅ Mighty Voice ready — hotkeys active");

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            set_app_state,
            set_api_key,
            get_api_key_status,
            update_hotkey,
            unregister_hotkey,
            list_input_devices,
            set_input_device,
            update_ocr_settings,
            update_ocr_hotkey,
            unregister_ocr_hotkey,
            update_screenshot_hotkey,
            unregister_screenshot_hotkey,
            run_ocr_capture_now,
            open_screenshot_overlay,
            screenshot_overlay_action
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
