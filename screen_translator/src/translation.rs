use reqwest;
use serde::{Deserialize, Serialize};
use base64::{Engine as _, engine::general_purpose};
use std::fs::OpenOptions;
use std::io::Write;

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<Content>,
}

#[derive(Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Serialize)]
struct Part {
    text: Option<String>,
    inline_data: Option<InlineData>,
}

#[derive(Serialize)]
struct InlineData {
    mime_type: String,
    data: String,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Vec<Candidate>,
}

#[derive(Deserialize)]
struct Candidate {
    content: ContentResponse,
}

#[derive(Deserialize)]
struct ContentResponse {
    parts: Vec<PartResponse>,
}

#[derive(Deserialize)]
struct PartResponse {
    text: String,
}

pub async fn translate_with_gemini_image(api_key: &str, prompt: &str, image_bytes: &[u8]) -> Result<String, anyhow::Error> {
    let mut log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("debug.txt")?;
    writeln!(log_file, "Starting Gemini translation request")?;

    let client = reqwest::Client::new();
    let b64 = general_purpose::STANDARD.encode(image_bytes);
    let request = GeminiRequest {
        contents: vec![Content {
            parts: vec![
                Part {
                    text: Some(prompt.to_string()),
                    inline_data: None,
                },
                Part {
                    text: None,
                    inline_data: Some(InlineData {
                        mime_type: "image/png".to_string(),
                        data: b64,
                    }),
                },
            ],
        }],
    };
    let url = format!("https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash-lite:generateContent?key={}", api_key);
    writeln!(log_file, "Sending request to URL: {}", url)?;
    let response = match client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            writeln!(log_file, "Response status: {}", status)?;
            if status.is_success() {
                resp
            } else {
                let body = resp.text().await.unwrap_or("Failed to read body".to_string());
                writeln!(log_file, "Error response body: {}", body)?;
                return Err(anyhow::anyhow!("API error: {} - {}", status, body));
            }
        }
        Err(e) => {
            writeln!(log_file, "Request send error: {}", e)?;
            return Err(e.into());
        }
    };

    match response.json::<GeminiResponse>().await {
        Ok(resp) => {
            if resp.candidates.is_empty() {
                writeln!(log_file, "No candidates in response")?;
                return Err(anyhow::anyhow!("No candidates in response"));
            }
            let text = resp.candidates[0].content.parts[0].text.trim().to_string();
            writeln!(log_file, "Extracted text: {}", text)?;
            Ok(text)
        }
        Err(e) => {
            writeln!(log_file, "JSON parse error: {}", e)?;
            Err(e.into())
        }
    }
}

pub async fn translate_with_groq(api_key: &str, prompt: &str, text: &str) -> Result<String, anyhow::Error> {
    let mut log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("debug.txt")?;
    writeln!(log_file, "Starting Groq translation request")?;

    let client = reqwest::Client::new();
    let full_prompt = format!("{}: {}", prompt, text);
    let request = serde_json::json!({
        "model": "meta-llama/llama-4-scout-17b-16e-instruct",
        "messages": [
            {
                "role": "user",
                "content": full_prompt
            }
        ],
        "temperature": 0.7
    });
    let url = "https://api.groq.com/openai/v1/chat/completions";
    writeln!(log_file, "Sending request to Groq URL")?;
    let response = match client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            writeln!(log_file, "Groq Response status: {}", status)?;
            if status.is_success() {
                resp
            } else {
                let body = resp.text().await.unwrap_or("Failed to read body".to_string());
                writeln!(log_file, "Groq Error response body: {}", body)?;
                return Err(anyhow::anyhow!("Groq API error: {} - {}", status, body));
            }
        }
        Err(e) => {
            writeln!(log_file, "Groq Request send error: {}", e)?;
            return Err(e.into());
        }
    };

    let resp_json: serde_json::Value = response.json().await?;
    if let Some(choices) = resp_json["choices"].as_array() {
        if let Some(choice) = choices.get(0) {
            if let Some(content) = choice["message"]["content"].as_str() {
                let translated = content.trim().to_string();
                writeln!(log_file, "Groq Extracted text: {}", translated)?;
                Ok(translated)
            } else {
                writeln!(log_file, "No content in Groq response")?;
                Err(anyhow::anyhow!("No content in Groq response"))
            }
        } else {
            writeln!(log_file, "No choices in Groq response")?;
            Err(anyhow::anyhow!("No choices in Groq response"))
        }
    } else {
        writeln!(log_file, "Invalid Groq response structure")?;
        Err(anyhow::anyhow!("Invalid Groq response structure"))
    }
}

pub async fn translate_with_groq_image(api_key: &str, prompt: &str, image_bytes: &[u8]) -> Result<String, anyhow::Error> {
    let mut log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("debug.txt")?;
    writeln!(log_file, "Starting Groq image translation request")?;

    let client = reqwest::Client::new();
    let b64 = general_purpose::STANDARD.encode(image_bytes);
    let request = serde_json::json!({
        "model": "meta-llama/llama-4-scout-17b-16e-instruct",
        "messages": [
            {
                "role": "user",
                "content": [
                    {
                        "type": "text",
                        "text": prompt
                    },
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:image/png;base64,{}", b64)
                        }
                    }
                ]
            }
        ],
        "temperature": 0.7
    });
    let url = "https://api.groq.com/openai/v1/chat/completions";
    writeln!(log_file, "Sending request to Groq URL")?;
    let response = match client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            writeln!(log_file, "Groq Response status: {}", status)?;
            if status.is_success() {
                resp
            } else {
                let body = resp.text().await.unwrap_or("Failed to read body".to_string());
                writeln!(log_file, "Groq Error response body: {}", body)?;
                return Err(anyhow::anyhow!("Groq API error: {} - {}", status, body));
            }
        }
        Err(e) => {
            writeln!(log_file, "Groq Request send error: {}", e)?;
            return Err(e.into());
        }
    };

    let resp_json: serde_json::Value = response.json().await?;
    if let Some(choices) = resp_json["choices"].as_array() {
        if let Some(choice) = choices.get(0) {
            if let Some(content) = choice["message"]["content"].as_str() {
                let translated = content.trim().to_string();
                writeln!(log_file, "Groq Extracted text: {}", translated)?;
                Ok(translated)
            } else {
                writeln!(log_file, "No content in Groq response")?;
                Err(anyhow::anyhow!("No content in Groq response"))
            }
        } else {
            writeln!(log_file, "No choices in Groq response")?;
            Err(anyhow::anyhow!("No choices in Groq response"))
        }
    } else {
        writeln!(log_file, "Invalid Groq response structure")?;
        Err(anyhow::anyhow!("Invalid Groq response structure"))
    }
}

pub async fn translate_from_image(api: &str, key: &str, prompt: &str, image_bytes: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    match api {
        "gemini" => translate_with_gemini_image(key, prompt, image_bytes).await.map_err(|e| e.into()),
        "groq" => translate_with_groq_image(key, prompt, image_bytes).await.map_err(|e| e.into()),
        _ => Err("Invalid API".into()),
    }
}