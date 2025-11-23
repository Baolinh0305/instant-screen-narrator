use reqwest;
use serde::{Deserialize, Serialize};
use base64::{Engine as _, engine::general_purpose};
use anyhow::Result;

// --- STRUCTURES ---
#[derive(Serialize)]
struct GeminiRequest { contents: Vec<Content> }
#[derive(Serialize)]
struct Content { parts: Vec<Part> }
#[derive(Serialize)]
struct Part { text: Option<String>, inline_data: Option<InlineData> }
#[derive(Serialize)]
struct InlineData { mime_type: String, data: String }
#[derive(Deserialize)]
struct GeminiResponse { candidates: Vec<Candidate> }
#[derive(Deserialize)]
struct Candidate { content: ContentResponse }
#[derive(Deserialize)]
struct ContentResponse { parts: Vec<PartResponse> }
#[derive(Deserialize)]
struct PartResponse { text: String }

pub struct TranslationResult {
    pub text: String,
    pub remaining_requests: Option<i32>,
}

#[derive(Debug)]
pub enum TranslationError {
    RateLimitExceeded,
    Other(anyhow::Error),
}

// --- GEMINI ---
pub async fn translate_with_gemini_image(api_key: &str, prompt: &str, image_bytes: &[u8]) -> Result<TranslationResult, TranslationError> {
    let client = reqwest::Client::new();
    let b64 = general_purpose::STANDARD.encode(image_bytes);
    let request = GeminiRequest {
        contents: vec![Content {
            parts: vec![
                Part { text: Some(prompt.to_string()), inline_data: None },
                Part { text: None, inline_data: Some(InlineData { mime_type: "image/png".to_string(), data: b64 }) },
            ],
        }],
    };
    // ĐÃ SỬA LẠI ĐÚNG MODEL BẠN YÊU CẦU: gemini-2.5-flash-lite
    let url = format!("https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash-lite:generateContent?key={}", api_key);
    
    let response = client.post(&url).header("Content-Type", "application/json").json(&request).send().await.map_err(|e| TranslationError::Other(e.into()))?;

    if !response.status().is_success() {
        let status = response.status();
        if status.as_u16() == 429 { return Err(TranslationError::RateLimitExceeded); }
        return Err(TranslationError::Other(anyhow::anyhow!("Gemini Error {}", status)));
    }

    let resp_json: GeminiResponse = response.json().await.map_err(|e| TranslationError::Other(e.into()))?;
    if resp_json.candidates.is_empty() { return Err(TranslationError::Other(anyhow::anyhow!("No candidates"))); }
    let text = resp_json.candidates[0].content.parts[0].text.trim().to_string();
    
    Ok(TranslationResult { text, remaining_requests: None })
}

// --- GROQ ---
pub async fn translate_with_groq_image(api_key: &str, prompt: &str, image_bytes: &[u8]) -> Result<TranslationResult, TranslationError> {
    let client = reqwest::Client::new();
    let b64 = general_purpose::STANDARD.encode(image_bytes);
    let request = serde_json::json!({
        "model": "meta-llama/llama-4-scout-17b-16e-instruct",
        "messages": [
            {
                "role": "user",
                "content": [
                    { "type": "text", "text": prompt },
                    { "type": "image_url", "image_url": { "url": format!("data:image/png;base64,{}", b64) } }
                ]
            }
        ],
        "temperature": 0.1
    });
    let url = "https://api.groq.com/openai/v1/chat/completions";
    
    let response = client.post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await
        .map_err(|e| TranslationError::Other(e.into()))?;

    let status = response.status();
    let remaining = response.headers().get("x-ratelimit-remaining-requests").and_then(|h| h.to_str().ok()).and_then(|s| s.parse::<i32>().ok());

    if !status.is_success() {
        if status.as_u16() == 429 { return Err(TranslationError::RateLimitExceeded); }
        let body = response.text().await.unwrap_or_default();
        return Err(TranslationError::Other(anyhow::anyhow!("Groq Error {}: {}", status, body)));
    }

    let resp_json: serde_json::Value = response.json().await.map_err(|e| TranslationError::Other(e.into()))?;
    
    if let Some(content) = resp_json["choices"][0]["message"]["content"].as_str() {
        Ok(TranslationResult { text: content.trim().to_string(), remaining_requests: remaining })
    } else {
        Err(TranslationError::Other(anyhow::anyhow!("Invalid Groq response")))
    }
}

pub async fn translate_from_image(api: &str, key: &str, prompt: &str, image_bytes: &[u8]) -> Result<TranslationResult, TranslationError> {
    match api {
        "gemini" => translate_with_gemini_image(key, prompt, image_bytes).await,
        "groq" => translate_with_groq_image(key, prompt, image_bytes).await,
        _ => Err(TranslationError::Other(anyhow::anyhow!("Invalid API"))),
    }
}