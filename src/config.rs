use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CustomPrompt {
    pub content: String, 
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    pub gemini_api_key: String,
    #[serde(default)]
    pub groq_api_keys: Vec<String>,
    #[serde(default)]
    pub active_groq_index: usize,

    pub current_prompt: String,
    
    #[serde(default)]
    pub saved_prompts: Vec<CustomPrompt>, 
    
    pub hotkey_translate: String,
    pub hotkey_select: String,
    pub hotkey_instant: String,
    pub hotkey_auto: String,
    pub split_tts: bool,
    pub use_tts: bool,
    pub show_overlay: bool,
    
    pub fixed_regions: Vec<Region>,
    pub arrow_region: Option<Region>,
    pub instant_region: Option<Region>,

    pub selected_api: String,
    pub speed: f32,

    #[serde(default = "default_interval")]
    pub arrow_check_interval: f32,
    #[serde(default)]
    pub auto_copy: bool,
    #[serde(default)]
    pub copy_instant_only: bool,
    
    // ĐÃ XÓA: minimize_to_tray
}

fn default_interval() -> f32 { 0.02 }

impl Default for Config {
    fn default() -> Self {
        Self {
            gemini_api_key: String::new(),
            groq_api_keys: Vec::new(),
            active_groq_index: 0,
            current_prompt: Self::get_normal_prompt(),
            saved_prompts: Vec::new(),
            hotkey_translate: "[".to_string(),
            hotkey_select: "]".to_string(),
            hotkey_instant: "\\".to_string(),
            hotkey_auto: ";".to_string(),
            split_tts: true,
            use_tts: true,
            show_overlay: false,
            fixed_regions: Vec::new(),
            arrow_region: None,
            instant_region: None,
            selected_api: "groq".to_string(),
            speed: 1.0,
            arrow_check_interval: 0.02,
            auto_copy: false,
            copy_instant_only: false,
        }
    }
}

impl Config {
    pub fn get_wuxia_prompt() -> String {
        "Perform OCR to extract all text from this image, regardless of the source language. Then, translate the extracted text into Vietnamese. The translation must strictly use vocabulary and tone consistent with wuxia novels, make it as short as possible. Crucially, provide ONLY the translated text and nothing else. Do not include any introductory phrases, explanations, or conversational elements. Note: just output the translated text and make it as short as possible".to_string()
    }

    pub fn get_wuxia_speaker_prompt() -> String {
        "Identify the character name at the beginning. Analyze the context/tone of the dialogue to choose a fitting Vietnamese verb (e.g., nói, hỏi, đáp, cười lạnh, quát...). Format the output strictly as: 'Name Verb: Vietnamese Translation'. Do NOT enclose the dialogue in quotation marks. Use wuxia novel vocabulary. Provide ONLY the translated result.".to_string()
    }

    pub fn get_normal_prompt() -> String {
        "Perform OCR to extract all text visible in this image, regardless of the original language. Then, translate the extracted text directly into Vietnamese. Return only the Vietnamese translation, no introduction or notes.".to_string()
    }
}

impl Config {
    pub fn get_config_dir() -> PathBuf {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".to_string());
        std::path::Path::new(&home).join(".screen_translator")
    }

    fn get_config_path() -> PathBuf {
        Self::get_config_dir().join("config.txt")
    }

    pub fn get_custom_arrow_path() -> PathBuf {
        Self::get_config_dir().join("custom_arrow.png")
    }

    pub fn load() -> Self {
        let path = Self::get_config_path();
        if path.exists() {
            match fs::read_to_string(path) {
                Ok(content) => {
                    let mut config: Config = serde_json::from_str(&content).unwrap_or_default();
                    if config.active_groq_index >= config.groq_api_keys.len() && !config.groq_api_keys.is_empty() {
                        config.active_groq_index = 0;
                    }
                    if config.arrow_check_interval < 0.02 { config.arrow_check_interval = 0.02; }
                    if config.arrow_check_interval > 0.2 { config.arrow_check_interval = 0.2; }
                    config
                },
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
    
    pub fn get_current_groq_key(&self) -> String {
        if self.groq_api_keys.is_empty() { return String::new(); }
        if self.active_groq_index < self.groq_api_keys.len() { self.groq_api_keys[self.active_groq_index].clone() } 
        else { self.groq_api_keys[0].clone() }
    }
}