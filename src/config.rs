use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Serialize, Deserialize, Clone)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    pub gemini_api_key: String,
    pub groq_api_key: String,
    pub current_prompt: String,
    pub custom_prompt: String,
    pub hotkey_translate: String,
    pub hotkey_select: String,
    pub hotkey_instant: String,
    pub split_tts: bool,
    pub use_tts: bool,
    pub show_overlay: bool,
    pub fixed_regions: Vec<Region>,
    pub selected_api: String,
    pub speed: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            gemini_api_key: String::new(),
            groq_api_key: String::new(),
            current_prompt: Self::get_normal_prompt(),
            custom_prompt: "Phân tích hình ảnh, trả về raw text, không định dạng, thật ngắn gọn".to_string(),
            hotkey_translate: "[".to_string(),
            hotkey_select: "]".to_string(),
            hotkey_instant: "\\".to_string(),
            split_tts: true,
            use_tts: true,
            show_overlay: false,
            fixed_regions: Vec::new(),
            selected_api: "groq".to_string(),
            speed: 1.0,
        }
    }
}

impl Config {
    pub fn get_wuxia_prompt() -> String {
        "Perform OCR to extract all text from this image, regardless of the source language. Then, translate the extracted text into Vietnamese. The translation must strictly use vocabulary and tone consistent with wuxia novels, make it as short as possible. Crucially, provide ONLY the translated text and nothing else. Do not include any introductory phrases, explanations, or conversational elements. Note: just output the translated text and make it as short as possible".to_string()
    }

    pub fn get_normal_prompt() -> String {
        "Perform OCR to extract all text visible in this image, regardless of the original language. Then, translate the extracted text directly into Vietnamese. Return only the Vietnamese translation, no introduction or notes.".to_string()
    }
}

impl Config {
    fn get_config_path() -> std::path::PathBuf {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".to_string());
        std::path::Path::new(&home).join(".screen_translator").join("config.txt")
    }

    pub fn load() -> Self {
        let path = Self::get_config_path();
        if path.exists() {
            match fs::read_to_string(path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(_) => Self::default(),
            }
        } else {
            Self::default()
        }
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let content = serde_json::to_string_pretty(self)?;
        let path = Self::get_config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)?;
        Ok(())
    }
}