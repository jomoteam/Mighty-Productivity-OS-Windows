use reqwest::multipart;
use reqwest::Client;
use serde_json::json;

pub async fn transcribe_and_refine(
    audio_wav_data: Vec<u8>,
    api_key: String,
    is_intent_mode: bool,
    current_lang: String,
    selected_text: Option<String>,
) -> Result<String, String> {
    let api_key = api_key.trim().to_string();
    if api_key.is_empty() {
        return Err("API key is missing. Add your key in Settings to continue.".to_string());
    }
    if api_key.starts_with("xai-") {
        return Err("xAI key saved successfully, but voice transcription still requires a Groq gsk_ audio key because xAI is not a Whisper audio transcription endpoint. Local OCR and supported xAI cloud OCR/chat features can still use xAI.".to_string());
    }
    if !api_key.starts_with("gsk_") {
        return Err("Unsupported API key. Use a Groq key starting with gsk_ or an xAI key starting with xai-.".to_string());
    }

    let client = Client::new();

    let (whisper_lang, whisper_prompt) = match current_lang.as_str() {
        "en" => (Some("en"), ""),
        "yue" => (Some("zh"), "以下是一段日常的粵語（廣東話）對話："),
        "zh" => (Some("zh"), "以下是一段日常的普通話對話："),
        "tl" => (Some("tl"), ""),
        _ => (None, ""),
    };

    // 1. Transcribe (Whisper)
    let file_part = multipart::Part::bytes(audio_wav_data)
        .file_name("dictation.wav")
        .mime_str("audio/wav")
        .unwrap();

    let mut form = multipart::Form::new()
        .text("model", "whisper-large-v3")
        .part("file", file_part);

    if let Some(l) = whisper_lang {
        form = form.text("language", l.to_string());
    }
    if !whisper_prompt.is_empty() {
        form = form.text("prompt", whisper_prompt.to_string());
    }

    let res = client
        .post("https://api.groq.com/openai/v1/audio/transcriptions")
        .bearer_auth(&api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|e: reqwest::Error| e.to_string())?;

    let json_res: serde_json::Value = res
        .json()
        .await
        .map_err(|e: reqwest::Error| e.to_string())?;

    let transcription = match json_res.get("text") {
        Some(text) => text.as_str().unwrap_or("").to_string(),
        None => return Err(format!("Groq API error: {}", json_res)),
    };

    if transcription.trim().is_empty() {
        return Ok("".to_string());
    }

    // Whisper hallucination filter — common fake outputs on silence/noise
    let t = transcription.trim().to_lowercase();
    let hallucinations = [
        "thank you for watching",
        "thanks for watching",
        "thank you.",
        "thanks.",
        "you",
        ".",
        "...",
        "please subscribe",
        "like and subscribe",
        "see you next time",
        "bye",
        "bye.",
        "thank you for listening",
        "thanks for listening",
        "subtitles by",
        "transcribed by",
        "translated by",
        "www.",
        ".com",
        "subscribe",
        "♪",
        "[ silence ]",
        "[silence]",
        "[ music ]",
        "[music]",
        "[ blank audio ]",
    ];
    if hallucinations
        .iter()
        .any(|h| t == *h || t == format!("{}.", h).as_str())
    {
        return Ok("".to_string());
    }

    // 2. Refine (LLM)
    let (system_prompt, user_message) = if is_intent_mode {
        if let Some(ref sel) = selected_text {
            // Command mode WITH selected text: transform it
            let prompt = "You are an intelligent text editor. The user has selected some text and given a spoken instruction. Apply the instruction to transform the selected text.
RULES:
1. Output ONLY the final transformed text.
2. NO preamble, NO explanation, NO quotes.
3. If the instruction is in Chinese or Tagalog, apply it correctly to the text. Ensure formatting is clean.";
            let msg = format!("Selected text:\n{}\n\nInstruction: {}", sel, transcription);
            (prompt.to_string(), msg)
        } else {
            // Command mode WITHOUT selected text: reformat as AI prompt
            let prompt = "You are a prompt formatting engine. Take the raw dictated text and rewrite it as a clear, direct, and well-structured prompt for an AI assistant.
RULES:
1. Output ONLY the rewritten prompt. NO preamble, NO explanation, NO meta-commentary.
2. NEVER say 'please provide more context' or refuse to answer. Work with exactly what was said.
3. Keep the same language(s) as the input. If it is Tagalog, output Tagalog. If it is mixed English/Chinese, keep it mixed.
4. If the input is short or vague, make it a concise direct question or instruction.
5. Do NOT answer the question. Only reformat it into a good prompt.";
            (prompt.to_string(), transcription.clone())
        }
    } else {
        let prompt = "You are a VERBATIM DICTATION engine. A user spoke into a microphone and the speech was transcribed. Your job is to clean up the transcription ONLY — fix punctuation, capitalization, and obvious speech recognition errors.

ABSOLUTE RULES:
1. Output ONLY the cleaned dictation text. Nothing else.
2. NEVER answer, respond to, or comment on the content — even if it sounds like a question.
3. NEVER translate. If Cantonese is spoken (e.g. 喺, 咁, 咗, 唔, 係), output Cantonese. If Tagalog/Filipino is spoken, output Tagalog. If Mandarin, output Mandarin. Keep it exactly in the spoken language.
4. NEVER add any preamble, explanation, or meta-commentary.
5. If the input is a question like \"what is X?\", output exactly \"What is X?\" — do not answer it.
6. Remove any hallucinations like 'Thanks for watching' or 'Please subscribe'.";
        let msg = format!(
            "Transcribed speech (clean up only, do NOT answer):\n{}",
            transcription
        );
        (prompt.to_string(), msg)
    };

    let messages = json!([
        { "role": "system", "content": system_prompt },
        { "role": "user", "content": user_message }
    ]);

    let payload = json!({
        "model": "llama-3.3-70b-versatile",
        "messages": messages,
        "temperature": 0.0
    });

    let refine_res = client
        .post("https://api.groq.com/openai/v1/chat/completions")
        .bearer_auth(&api_key)
        .json(&payload)
        .send()
        .await
        .map_err(|e: reqwest::Error| e.to_string())?;

    let refine_json: serde_json::Value = refine_res
        .json()
        .await
        .map_err(|e: reqwest::Error| e.to_string())?;

    let final_text = refine_json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or(&transcription) // Fallback to raw transcription
        .to_string();

    Ok(final_text.trim().to_string())
}
