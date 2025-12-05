// ... (Giữ nguyên các phần import ở đầu file)
use crate::config;
use crate::overlay;
use eframe::egui;
use std::sync::atomic::Ordering;
use crate::AUTO_TRANSLATE_ENABLED;
use crate::HOTKEYS_NEED_UPDATE;
use crate::BindingTarget;
use crate::IS_BINDING_MODE;
use crate::KEY_STATE_MASK;
use crate::VK_MIN;
use crate::VK_MAX;
use crate::BINDING_SLEEP_MS;
use crate::FONT_SIZE_MIN;
use crate::FONT_SIZE_MAX;
use crate::TTS_SPEED_MIN;
use crate::TTS_SPEED_MAX;
use crate::ARROW_CHECK_INTERVAL_MIN;
use crate::ARROW_CHECK_INTERVAL_MAX;
use crate::DEFAULT_ARROW_CHECK_INTERVAL;
use crate::SUCCESS_DISPLAY_DURATION_SECS;
use crate::WWM_TEXT_REGION_X_RATIO;
use crate::WWM_TEXT_REGION_Y_RATIO;
use crate::WWM_TEXT_REGION_W_RATIO;
use crate::WWM_TEXT_REGION_H_RATIO;
use crate::WWM_NAME_REGION_X_RATIO;
use crate::WWM_NAME_REGION_Y_RATIO;
use crate::WWM_NAME_REGION_W_RATIO;
use crate::WWM_NAME_REGION_H_RATIO;
use crate::WWM_ARROW_REGION_X_RATIO;
use crate::WWM_ARROW_REGION_Y_RATIO;
use crate::WWM_ARROW_REGION_W_RATIO;
use crate::WWM_ARROW_REGION_H_RATIO;
use crate::WWM_REGION_PADDING;
use crate::WWM_REGION_EXTRA_WIDTH;
use crate::WWM_REGION_EXTRA_HEIGHT;
use crate::APP_NAME;
use winapi::um::winuser::{
    GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN, FindWindowW, GetClientRect, ClientToScreen,
    MessageBoxW, MB_OK, MB_ICONWARNING, MB_TOPMOST, IsWindowVisible
};
use winapi::shared::windef::{RECT, POINT};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use webbrowser;
use std::fs;
use rfd;

// ... (Giữ nguyên các hàm helper to_wide, get_game_bounds, show_alert, ReaderState, UiState...)
fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

fn get_game_bounds(window_title: &str) -> Option<(i32, i32, u32, u32)> {
    unsafe {
        let wide_title = to_wide(window_title);
        let hwnd = FindWindowW(std::ptr::null(), wide_title.as_ptr());
        if hwnd.is_null() || IsWindowVisible(hwnd) == 0 { return None; }
        let mut rect: RECT = std::mem::zeroed();
        if GetClientRect(hwnd, &mut rect) == 0 { return None; }
        let mut point = POINT { x: 0, y: 0 };
        if ClientToScreen(hwnd, &mut point) == 0 { return None; }
        let width = (rect.right - rect.left) as u32;
        let height = (rect.bottom - rect.top) as u32;
        Some((point.x, point.y, width, height))
    }
}

fn show_alert(message: &str) {
    let msg = message.to_string();
    std::thread::spawn(move || {
        unsafe {
            let wide_msg = to_wide(&msg);
            let wide_title = to_wide("Thông báo");
            MessageBoxW(std::ptr::null_mut(), wide_msg.as_ptr(), wide_title.as_ptr(), MB_OK | MB_ICONWARNING | MB_TOPMOST);
        }
    });
}

#[derive(Clone)]
pub struct ReaderState {
    pub is_open: bool,
    pub raw_text: String,
    pub chunks: Vec<String>,
    pub current_index: usize,
    pub is_playing: bool,
    pub processing_audio: bool,
}

impl ReaderState {
    fn new() -> Self {
        Self { is_open: false, raw_text: String::new(), chunks: Vec::new(), current_index: 0, is_playing: false, processing_audio: false, }
    }
    pub fn parse_text(&mut self) {
        let text = self.raw_text.clone();
        let mut final_chunks = Vec::new();
        let mut current_sentence = String::new();
        let mut in_quote = false;
        for c in text.chars() {
            if c == '"' || c == '“' || c == '”' { in_quote = !in_quote; }
            current_sentence.push(c);
            if !in_quote && (c == '.' || c == '!' || c == '?' || c == '\n') {
                if !current_sentence.trim().is_empty() { final_chunks.push(current_sentence.trim().to_string()); current_sentence.clear(); }
            }
        }
        if !current_sentence.trim().is_empty() { final_chunks.push(current_sentence.trim().to_string()); }
        self.chunks = final_chunks;
        self.current_index = 0;
        self.is_playing = false;
    }
}

#[derive(Clone)]
pub struct UiState {
    pub show_popup: bool,
    pub popup_text: String,
    pub show_reset_confirm: bool,
    pub show_arrow_window: bool,
    pub show_arrow_help: bool,
    pub show_password: bool,
    pub reader: ReaderState,
}

#[derive(Clone)]
pub struct ConfigState {
    pub config: config::Config,
    pub gemini_api_key: String,
    pub current_prompt: String,
    pub editing_prompt_index: Option<usize>,
    pub selected_api: String,
    pub use_tts: bool,
}

#[derive(Clone)]
pub struct HotkeyState {
    pub hotkey_translate: String,
    pub hotkey_select: String,
    pub hotkey_instant: String,
    pub hotkey_auto: String,
    pub hotkey_toggle_auto: String,
}

#[derive(Clone)]
pub struct WwmState {
    pub wwm_success_timer: Option<std::time::Instant>,
    pub wwm_name_success_timer: Option<std::time::Instant>,
    pub arrow_wwm_success_timer: Option<std::time::Instant>,
    pub auto_translate_active: bool,
}

impl UiState {
    pub fn new() -> Self {
        Self { show_popup: false, popup_text: String::new(), show_reset_confirm: false, show_arrow_window: false, show_arrow_help: false, show_password: false, reader: ReaderState::new(), }
    }
}

impl ConfigState {
    pub fn new(config: config::Config) -> Self {
        Self { gemini_api_key: config.gemini_api_key.clone(), current_prompt: config.current_prompt.clone(), editing_prompt_index: None, selected_api: config.selected_api.clone(), use_tts: config.use_tts, config, }
    }
}

impl HotkeyState {
    pub fn new(config: &config::Config) -> Self {
        Self { hotkey_translate: config.hotkey_translate.clone(), hotkey_select: config.hotkey_select.clone(), hotkey_instant: config.hotkey_instant.clone(), hotkey_auto: config.hotkey_auto.clone(), hotkey_toggle_auto: config.hotkey_toggle_auto.clone(), }
    }
}

impl WwmState {
    pub fn new() -> Self {
        Self { wwm_success_timer: None, wwm_name_success_timer: None, arrow_wwm_success_timer: None, auto_translate_active: false, }
    }
}

pub trait UiRenderer {
    fn render_header(&mut self, ui: &mut egui::Ui);
    fn render_api_section(&mut self, ui: &mut egui::Ui);
    fn render_prompt_section(&mut self, ui: &mut egui::Ui);
    fn render_hotkeys_section(&mut self, ui: &mut egui::Ui);
    fn render_aux_regions_section(&mut self, ui: &mut egui::Ui);
    fn render_settings_section(&mut self, ui: &mut egui::Ui);
    fn render_wwm_section(&mut self, ctx: &egui::Context, ui: &mut egui::Ui);
    fn render_reader_window(&mut self, ctx: &egui::Context);
    fn sync_config_from_file(&mut self);
    fn check_key_binding(&mut self);
    fn load_texture(&mut self, ctx: &egui::Context, bytes: &[u8], is_arrow: bool);
}

impl UiRenderer for super::MainApp {
    fn render_header(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.heading(egui::RichText::new(APP_NAME).strong().size(24.0));
        });
        ui.add_space(5.0);
        ui.horizontal(|ui| {
            let mut listening = !self.is_paused;
            if ui.checkbox(&mut listening, "✅ Nghe phím").changed() {
                self.is_paused = !listening;
                crate::LISTENING_PAUSED.store(self.is_paused, Ordering::Relaxed);
                if self.is_paused {
                    self.wwm_state.auto_translate_active = false;
                    AUTO_TRANSLATE_ENABLED.store(false, Ordering::Relaxed);
                }
            }
            if ui.button("🔽 Ẩn").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
            ui.separator();
            if ui.button(egui::RichText::new("📖 Đọc văn bản").small().strong()).clicked() {
                 self.ui_state.reader.is_open = true;
            }
            let theme_text = if self.config_state.config.is_dark_mode { "🌗 Tối" } else { "🌗 Sáng" };
            if ui.button(egui::RichText::new(theme_text).small()).clicked() {
                self.config_state.config.is_dark_mode = !self.config_state.config.is_dark_mode;
                self.config_state.config.save().unwrap();
            }
            if ui.button(egui::RichText::new("🔄 Reset").small().color(egui::Color32::RED)).clicked() {
                self.ui_state.show_reset_confirm = true;
            }
            if ui.button(egui::RichText::new("👤 by Baolinh0305").small()).clicked() {
                  let _ = webbrowser::open("https://github.com/Baolinh0305/instant-screen-narrator/releases");
            }
        });
        ui.add_space(5.0);
    }

    fn render_api_section(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(egui::RichText::new("🌐 Cấu hình API").strong()).default_open(true).show(ui, |ui| {
            egui::Grid::new("api_grid").num_columns(2).spacing([20.0, 15.0]).striped(true).show(ui, |ui| {
                ui.label("Dịch vụ:");
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_source("api_selector")
                        .selected_text(if self.config_state.selected_api == "gemini" { "Gemini (Không nên dùng)" } else { "Groq (Nhanh)" })
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            if ui.selectable_value(&mut self.config_state.selected_api, "gemini".to_string(), "Gemini (Không nên dùng)").clicked() {
                                self.config_state.config.selected_api = self.config_state.selected_api.clone();
                                self.config_state.config.save().unwrap();
                            }
                            if ui.selectable_value(&mut self.config_state.selected_api, "groq".to_string(), "Groq (Meta Llama)").clicked() {
                                self.config_state.config.selected_api = self.config_state.selected_api.clone();
                                self.config_state.config.save().unwrap();
                            }
                        });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let help_text = if self.config_state.selected_api == "gemini" { "❓ Hướng dẫn (Gemini)" } else { "❓ Hướng dẫn (Groq)" };
                        if ui.add(egui::Button::new(egui::RichText::new(help_text).small())).clicked() {
                            self.ui_state.show_popup = true;
                            self.ui_state.popup_text = self.config_state.selected_api.clone();
                        }
                    });
                });
                ui.end_row();

                ui.label("API Key:");
                ui.vertical(|ui| {
                      let show_pass = self.ui_state.show_password;
                      if self.config_state.selected_api == "gemini" {
                          if ui.add(egui::TextEdit::singleline(&mut self.config_state.gemini_api_key).password(!show_pass).desired_width(250.0)).changed() {
                              self.config_state.config.gemini_api_key = self.config_state.gemini_api_key.clone();
                              self.config_state.config.save().unwrap();
                          }
                      } else {
                          if self.config_state.config.groq_api_keys.is_empty() {
                              self.config_state.config.groq_api_keys.push(String::new());
                          }
                          if let Some(key) = self.config_state.config.groq_api_keys.get_mut(0) {
                              if ui.add(egui::TextEdit::singleline(key).password(!show_pass).desired_width(250.0)).changed() {
                                  self.config_state.config.save().unwrap();
                              }
                          }
                      }
                      if ui.button(if self.ui_state.show_password { "🙈 Ẩn Key" } else { "👁 Hiện Key" }).clicked() {
                          self.ui_state.show_password = !self.ui_state.show_password;
                      }
                 });
                 ui.end_row();
            });
        });
    }

    fn render_prompt_section(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(egui::RichText::new("📝 Cấu hình Dịch (Prompt)").strong()).default_open(true).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("🗡️ Kiếm hiệp").clicked() {
                    self.config_state.current_prompt = config::Config::get_wuxia_prompt();
                    self.config_state.config.current_prompt = self.config_state.current_prompt.clone();
                    self.config_state.editing_prompt_index = None;
                    self.config_state.config.save().unwrap();
                }
                if ui.button("🌍 Thông thường").clicked() {
                    self.config_state.current_prompt = config::Config::get_normal_prompt();
                    self.config_state.config.current_prompt = self.config_state.current_prompt.clone();
                    self.config_state.editing_prompt_index = None;
                    self.config_state.config.save().unwrap();
                }
                if ui.button("🔍 Phân tích hình ảnh").clicked() {
                    self.config_state.current_prompt = config::Config::get_analyze_prompt();
                    self.config_state.config.current_prompt = self.config_state.current_prompt.clone();
                    self.config_state.editing_prompt_index = None;
                    self.config_state.config.save().unwrap();
                }
                if ui.button("abc Xuất chữ").clicked() {
                    self.config_state.current_prompt = config::Config::get_extract_text_prompt();
                    self.config_state.config.current_prompt = self.config_state.current_prompt.clone();
                    self.config_state.editing_prompt_index = None;
                    self.config_state.config.save().unwrap();
                }
                let mut to_select = None;
                for (i, _) in self.config_state.config.saved_prompts.iter().enumerate() {
                    let btn_label = format!("Mẫu {}", i + 1);
                    let is_selected = self.config_state.editing_prompt_index == Some(i);
                    if ui.add(egui::Button::new(btn_label).selected(is_selected)).clicked() {
                        to_select = Some(i);
                    }
                }
                if let Some(i) = to_select {
                    self.config_state.editing_prompt_index = Some(i);
                    self.config_state.current_prompt = self.config_state.config.saved_prompts[i].content.clone();
                    self.config_state.config.current_prompt = self.config_state.current_prompt.clone();
                    self.config_state.config.save().unwrap();
                }
                if ui.button("➕").clicked() {
                    self.config_state.config.saved_prompts.push(config::CustomPrompt {
                        content: String::new(),
                    });
                    self.config_state.editing_prompt_index = Some(self.config_state.config.saved_prompts.len() - 1);
                    self.config_state.current_prompt = String::new();
                    self.config_state.config.save().unwrap();
                }
            });
            ui.add_space(5.0);
            if let Some(idx) = self.config_state.editing_prompt_index {
                if idx < self.config_state.config.saved_prompts.len() {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("Đang sửa: Mẫu {}", idx + 1)).italics());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(egui::Button::new(egui::RichText::new("🗑").color(egui::Color32::RED))).clicked() {
                                self.config_state.config.saved_prompts.remove(idx);
                                self.config_state.editing_prompt_index = None;
                                self.config_state.current_prompt = config::Config::get_normal_prompt();
                                self.config_state.config.current_prompt = self.config_state.current_prompt.clone();
                                self.config_state.config.save().unwrap();
                            }
                        });
                    });
                }
            }
            if ui.add(egui::TextEdit::multiline(&mut self.config_state.current_prompt).desired_rows(4).desired_width(f32::INFINITY)).changed() {
                self.config_state.config.current_prompt = self.config_state.current_prompt.clone();
                self.config_state.config.save().unwrap();
                if let Some(idx) = self.config_state.editing_prompt_index {
                    if idx < self.config_state.config.saved_prompts.len() {
                        self.config_state.config.saved_prompts[idx].content = self.config_state.current_prompt.clone();
                        self.config_state.config.save().unwrap();
                    }
                }
            }
        });
    }

    fn render_hotkeys_section(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(egui::RichText::new("⌨️ Phím tắt chung").strong()).default_open(true).show(ui, |ui| {
              egui::Grid::new("hotkey_grid").num_columns(2).spacing([20.0, 10.0]).striped(true).show(ui, |ui| {
                let mut draw_bind_btn = |label: &str, target: BindingTarget, current_key: &str| {
                      ui.label(label);
                      let btn_text = if self.binding_target == Some(target) { "🛑 Đang chờ phím..." } else { current_key };
                      let btn = if self.binding_target == Some(target) {
                          egui::Button::new(egui::RichText::new(btn_text).color(egui::Color32::YELLOW))
                      } else {
                          egui::Button::new(btn_text)
                      };
                      if ui.add(btn).clicked() {
                          if self.binding_target == Some(target) {
                              self.binding_target = None;
                              IS_BINDING_MODE.store(false, Ordering::Relaxed);
                          } else {
                              self.binding_target = Some(target);
                              IS_BINDING_MODE.store(true, Ordering::Relaxed);
                          }
                      }
                      ui.end_row();
                 };
                 draw_bind_btn("Dịch vùng đã chọn:", BindingTarget::Translate, &self.hotkey_state.hotkey_translate);
                 draw_bind_btn("Chọn vùng dịch:", BindingTarget::Select, &self.hotkey_state.hotkey_select);
                 draw_bind_btn("Chụp & Dịch ngay:", BindingTarget::Instant, &self.hotkey_state.hotkey_instant);
                 draw_bind_btn("Bật/Tắt Tự động dịch:", BindingTarget::ToggleAuto, &self.hotkey_state.hotkey_toggle_auto);
            });
        });
    }

    fn render_aux_regions_section(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(egui::RichText::new("📑 Vùng dịch phụ (Multi-Region)").strong()).default_open(true).show(ui, |ui| {
            if ui.button("➕ Thêm vùng dịch mới").clicked() {
                let new_id = self.config_state.config.aux_regions.len();
                self.config_state.config.aux_regions.push(config::AuxRegion {
                    id: new_id,
                    name: format!("Vùng phụ #{}", new_id + 1),
                    region: None,
                    hotkey_select: "NONE".to_string(),
                    hotkey_translate: "NONE".to_string(),
                });
                self.config_state.config.save().unwrap();
                HOTKEYS_NEED_UPDATE.store(true, Ordering::Relaxed);
            }
            ui.add_space(5.0);
            let mut remove_idx = None;
            for (i, aux) in self.config_state.config.aux_regions.iter_mut().enumerate() {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&aux.name).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("🗑 Xóa").clicked() { remove_idx = Some(i); }
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.label("Chọn:");
                        let btn_txt_sel = if self.binding_target == Some(BindingTarget::AuxSelect(i)) { "..." } else { &aux.hotkey_select };
                        if ui.button(btn_txt_sel).clicked() {
                            self.binding_target = Some(BindingTarget::AuxSelect(i));
                            IS_BINDING_MODE.store(true, Ordering::Relaxed);
                        }
                        ui.label("Dịch:");
                        let btn_txt_trans = if self.binding_target == Some(BindingTarget::AuxTranslate(i)) { "..." } else { &aux.hotkey_translate };
                        if ui.button(btn_txt_trans).clicked() {
                            self.binding_target = Some(BindingTarget::AuxTranslate(i));
                            IS_BINDING_MODE.store(true, Ordering::Relaxed);
                        }
                        if aux.region.is_some() { ui.label("✅ Đã có vùng"); } else { ui.label("⚠️ Chưa chọn vùng"); }
                    });
                });
                ui.add_space(2.0);
            }
            if let Some(i) = remove_idx {
                self.config_state.config.aux_regions.remove(i);
                self.config_state.config.save().unwrap();
                HOTKEYS_NEED_UPDATE.store(true, Ordering::Relaxed);
            }
        });
    }

    fn render_settings_section(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(egui::RichText::new("⚙️ Cài đặt hiển thị & Âm thanh").strong()).default_open(true).show(ui, |ui| {
            egui::Grid::new("settings_grid").num_columns(2).spacing([20.0, 10.0]).show(ui, |ui| {
                ui.label("Overlay:");
                ui.vertical(|ui| {
                    if ui.add(egui::Checkbox::new(&mut self.config_state.config.show_overlay, "Hiện văn bản")).changed() {
                        self.config_state.config.save().unwrap();
                    }
                    ui.horizontal(|ui| {
                        ui.label("Cỡ chữ:");
                        if ui.add_enabled(self.config_state.config.show_overlay, egui::Slider::new(&mut self.config_state.config.overlay_font_size, FONT_SIZE_MIN as i32..=FONT_SIZE_MAX as i32).text("px")).changed() {
                            overlay::set_font_size(self.config_state.config.overlay_font_size);
                            self.config_state.config.save().unwrap();
                        }
                    });
                });
                ui.end_row();
                ui.label("TTS (Đọc):");
                ui.vertical(|ui| {
                    if ui.add(egui::Checkbox::new(&mut self.config_state.use_tts, "Bật đọc")).changed() {
                        self.config_state.config.use_tts = self.config_state.use_tts;
                        self.config_state.config.save().unwrap();
                    }
                    ui.horizontal(|ui| {
                        ui.label("Tốc độ:");
                        if ui.add_enabled(self.config_state.use_tts, egui::Slider::new(&mut self.config_state.config.speed, TTS_SPEED_MIN..=TTS_SPEED_MAX).text("x")).changed() {
                            self.config_state.config.save().unwrap();
                        }
                    });
                });
                ui.end_row();
                ui.label("Tùy chọn khác:");
                ui.vertical(|ui| {
                    if ui.add(egui::Checkbox::new(&mut self.config_state.config.freeze_screen, "Đóng băng khi chọn vùng")).changed() {
                        self.config_state.config.save().unwrap();
                    }
                });
                ui.end_row();
                ui.label("Copy Text:");
                ui.vertical(|ui| {
                    if ui.add(egui::Checkbox::new(&mut self.config_state.config.auto_copy, "Tự động Copy kết quả")).changed() {
                        self.config_state.config.save().unwrap();
                    }
                    if self.config_state.config.auto_copy {
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut self.config_state.config.copy_original, false, "Copy bản dịch");
                            ui.radio_value(&mut self.config_state.config.copy_original, true, "Copy bản gốc");
                        });
                        if ui.add(egui::Checkbox::new(&mut self.config_state.config.copy_instant_only, "Chỉ áp dụng lên Dịch nhanh")).changed() {
                            self.config_state.config.save().unwrap();
                        }
                    }
                });
                ui.end_row();
            });
        });
    }

    fn render_wwm_section(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            egui::CollapsingHeader::new(egui::RichText::new("🎮 Dịch Where Winds Meet").strong()).default_open(true).show(ui, |ui| {
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    ui.label("Phím tắt chọn vùng Mũi tên:");
                    let btn_text = if self.binding_target == Some(BindingTarget::Auto) { "🛑 Chờ..." } else { &self.hotkey_state.hotkey_auto };
                    let btn = if self.binding_target == Some(BindingTarget::Auto) { egui::Button::new(egui::RichText::new(btn_text).color(egui::Color32::YELLOW)) } else { egui::Button::new(btn_text) };
                    if ui.add(btn).clicked() {
                        if self.binding_target == Some(BindingTarget::Auto) { self.binding_target = None; IS_BINDING_MODE.store(false, Ordering::Relaxed); } else { self.binding_target = Some(BindingTarget::Auto); IS_BINDING_MODE.store(true, Ordering::Relaxed); }
                    }
                    ui.add_space(10.0);
                    if ui.button("❓").clicked() { self.ui_state.show_arrow_help = true; }
                });
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Center), |ui| {
                        let mut wwm_text = "🎯 Tự động chọn vùng dịch WWM";
                        if let Some(time) = self.wwm_state.wwm_success_timer {
                            if time.elapsed().as_secs_f32() < SUCCESS_DISPLAY_DURATION_SECS { wwm_text = "✅ Đã chọn"; ctx.request_repaint(); }
                            else { self.wwm_state.wwm_success_timer = None; }
                        }
                        if ui.add(egui::Button::new(wwm_text)).clicked() {
                            if let Some((win_x, win_y, win_w, win_h)) = get_game_bounds("Where Winds Meet") {
                                let f_w = win_w as f32;
                                let f_h = win_h as f32;
                                let region = config::Region {
                                    x: win_x + (f_w * WWM_TEXT_REGION_X_RATIO) as i32 - WWM_REGION_PADDING,
                                    y: win_y + (f_h * WWM_TEXT_REGION_Y_RATIO) as i32 - WWM_REGION_PADDING,
                                    width: (f_w * WWM_TEXT_REGION_W_RATIO) as u32 + WWM_REGION_EXTRA_WIDTH,
                                    height: (f_h * WWM_TEXT_REGION_H_RATIO) as u32 + WWM_REGION_EXTRA_HEIGHT
                                };
                                self.config_state.config.fixed_regions.clear();
                                self.config_state.config.fixed_regions.push(region.clone());
                                self.config_state.current_prompt = config::Config::get_wuxia_prompt();
                                self.config_state.config.current_prompt = self.config_state.current_prompt.clone();
                                self.config_state.editing_prompt_index = None;
                                self.config_state.config.save().unwrap();
                                self.sync_config_from_file();
                                self.wwm_state.wwm_success_timer = Some(std::time::Instant::now());
                                overlay::show_highlight(RECT{left: region.x, top: region.y, right: region.x + region.width as i32, bottom: region.y + region.height as i32});
                            } else {
                                show_alert("Không tìm thấy cửa sổ 'Where Winds Meet'. Vui lòng mở game trước.");
                            }
                        }
                        ui.label(egui::RichText::new("(16:9)").italics().color(egui::Color32::GRAY));
                    });
                });
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Center), |ui| {
                        let mut wwm_name_text = "🎯 Tự chọn vùng dịch WWM (có tên người thoại)";
                        if let Some(time) = self.wwm_state.wwm_name_success_timer {
                            if time.elapsed().as_secs_f32() < SUCCESS_DISPLAY_DURATION_SECS { wwm_name_text = "✅ Đã chọn"; ctx.request_repaint(); }
                            else { self.wwm_state.wwm_name_success_timer = None; }
                        }
                        if ui.add(egui::Button::new(wwm_name_text)).clicked() {
                            if let Some((win_x, win_y, win_w, win_h)) = get_game_bounds("Where Winds Meet") {
                                let f_w = win_w as f32;
                                let f_h = win_h as f32;
                                let region = config::Region {
                                    x: win_x + (f_w * WWM_NAME_REGION_X_RATIO) as i32,
                                    y: win_y + (f_h * WWM_NAME_REGION_Y_RATIO) as i32,
                                    width: (f_w * WWM_NAME_REGION_W_RATIO) as u32,
                                    height: (f_h * WWM_NAME_REGION_H_RATIO) as u32
                                };
                                self.config_state.config.fixed_regions.clear();
                                self.config_state.config.fixed_regions.push(region.clone());
                                self.config_state.current_prompt = config::Config::get_wuxia_speaker_prompt();
                                self.config_state.config.current_prompt = self.config_state.current_prompt.clone();
                                self.config_state.editing_prompt_index = None;
                                self.config_state.config.save().unwrap();
                                self.sync_config_from_file();
                                self.wwm_state.wwm_name_success_timer = Some(std::time::Instant::now());
                                overlay::show_highlight(RECT{left: region.x, top: region.y, right: region.x + region.width as i32, bottom: region.y + region.height as i32});
                            } else {
                                show_alert("Không tìm thấy cửa sổ 'Where Winds Meet'.");
                            }
                        }
                        ui.label(egui::RichText::new("(16:9)").italics().color(egui::Color32::GRAY));
                    });
                });
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Center), |ui| {
                        let mut arrow_text = "🏹 Tự động chọn vùng Mũi tên WWM";
                        if let Some(time) = self.wwm_state.arrow_wwm_success_timer {
                            if time.elapsed().as_secs_f32() < SUCCESS_DISPLAY_DURATION_SECS { arrow_text = "✅ Đã chọn"; ctx.request_repaint(); }
                            else { self.wwm_state.arrow_wwm_success_timer = None; }
                        }
                        if ui.add(egui::Button::new(arrow_text)).clicked() {
                            if let Some((win_x, win_y, win_w, win_h)) = get_game_bounds("Where Winds Meet") {
                                let f_w = win_w as f32;
                                let f_h = win_h as f32;
                                let region = config::Region {
                                    x: win_x + (f_w * WWM_ARROW_REGION_X_RATIO) as i32,
                                    y: win_y + (f_h * WWM_ARROW_REGION_Y_RATIO) as i32,
                                    width: (f_w * WWM_ARROW_REGION_W_RATIO) as u32,
                                    height: (f_h * WWM_ARROW_REGION_H_RATIO) as u32
                                };
                                self.config_state.config.arrow_region = Some(region.clone());
                                self.config_state.config.save().unwrap();
                                self.sync_config_from_file();
                                self.wwm_state.arrow_wwm_success_timer = Some(std::time::Instant::now());
                                overlay::show_highlight(RECT{left: region.x, top: region.y, right: region.x + region.width as i32, bottom: region.y + region.height as i32});
                            } else {
                                show_alert("Không tìm thấy cửa sổ 'Where Winds Meet'.");
                            }
                        }
                        ui.label(egui::RichText::new("(16:9)").italics().color(egui::Color32::GRAY));
                        if ui.button("🖼️").clicked() { self.ui_state.show_arrow_window = true; }
                    });
                });

                // --- ĐÃ ẨN SLIDER TỐC ĐỘ MŨI TÊN TẠI ĐÂY ---

                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Center), |ui| {
                        let btn_text = if self.wwm_state.auto_translate_active { "🔄 ĐANG BẬT TỰ ĐỘNG DỊCH" } else { "🔄 Bật Tự Động Dịch" };
                        let btn_color = if self.wwm_state.auto_translate_active { egui::Color32::DARK_GREEN } else { egui::Color32::from_rgb(60, 60, 60) };
                        if ui.add(egui::Button::new(egui::RichText::new(btn_text).strong().color(egui::Color32::WHITE)).fill(btn_color).min_size(egui::vec2(200.0, 30.0))).clicked() {
                            self.wwm_state.auto_translate_active = !self.wwm_state.auto_translate_active;
                            AUTO_TRANSLATE_ENABLED.store(self.wwm_state.auto_translate_active, Ordering::Relaxed);
                            crate::show_toggle_notification(self.wwm_state.auto_translate_active);
                        }
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Center), |ui| {
                        if ui.button("🐞 Debug Overlay").clicked() {
                            overlay::toggle_debug_overlay();
                        }
                        if ui.button("📂 Đổi ảnh mũi tên").clicked() {
                            if let Some(path) = rfd::FileDialog::new().add_filter("Image", &["png"]).pick_file() {
                                if let Ok(bytes) = fs::read(&path) {
                                    let dest = config::Config::get_custom_arrow_path();
                                    if let Ok(_) = fs::write(&dest, &bytes) {
                                        self.load_texture(ctx, &bytes, true);
                                    }
                                }
                            }
                        }
                        let custom_path = config::Config::get_custom_arrow_path();
                        if custom_path.exists() {
                            if ui.button("❌ Reset Mặc định").clicked() {
                                let _ = fs::remove_file(custom_path);
                                self.load_texture(ctx, crate::DEFAULT_ARROW, true);
                            }
                        }
                    });
                });
            });
        });
    }

    fn sync_config_from_file(&mut self) {
        let new_config = config::Config::load();
        self.config_state.config.fixed_regions = new_config.fixed_regions;
        self.config_state.config.arrow_region = new_config.arrow_region;
        self.config_state.config.instant_region = new_config.instant_region;
        self.config_state.config.aux_regions = new_config.aux_regions;
    }

    fn check_key_binding(&mut self) {
        if let Some(target) = self.binding_target {
            unsafe {
                for vk in VK_MIN..VK_MAX {
                    if (winapi::um::winuser::GetAsyncKeyState(vk) as u16 & KEY_STATE_MASK) != 0 {
                        if vk == winapi::um::winuser::VK_LBUTTON || vk == winapi::um::winuser::VK_RBUTTON || vk == winapi::um::winuser::VK_MBUTTON { continue; }
                        let key_name = crate::key_utils::get_name_from_vk(vk);
                        match target {
                            BindingTarget::Translate => { self.hotkey_state.hotkey_translate = key_name.clone(); self.config_state.config.hotkey_translate = key_name; }
                            BindingTarget::Select => { self.hotkey_state.hotkey_select = key_name.clone(); self.config_state.config.hotkey_select = key_name; }
                            BindingTarget::Instant => { self.hotkey_state.hotkey_instant = key_name.clone(); self.config_state.config.hotkey_instant = key_name; }
                            BindingTarget::Auto => { self.hotkey_state.hotkey_auto = key_name.clone(); self.config_state.config.hotkey_auto = key_name; }
                            BindingTarget::ToggleAuto => { self.hotkey_state.hotkey_toggle_auto = key_name.clone(); self.config_state.config.hotkey_toggle_auto = key_name; }
                            BindingTarget::AuxSelect(idx) => { if idx < self.config_state.config.aux_regions.len() { self.config_state.config.aux_regions[idx].hotkey_select = key_name; } }
                            BindingTarget::AuxTranslate(idx) => { if idx < self.config_state.config.aux_regions.len() { self.config_state.config.aux_regions[idx].hotkey_translate = key_name; } }
                        }
                        self.config_state.config.save().unwrap();
                        HOTKEYS_NEED_UPDATE.store(true, Ordering::Relaxed);
                        self.binding_target = None;
                        IS_BINDING_MODE.store(false, Ordering::Relaxed);
                        std::thread::sleep(std::time::Duration::from_millis(BINDING_SLEEP_MS));
                        break;
                    }
                }
            }
        }
    }

    fn render_reader_window(&mut self, ctx: &egui::Context) {
        if !self.ui_state.reader.is_open { return; }
        let mut open = true;
        egui::Window::new("Trình đọc văn bản (Text to Speech)").open(&mut open).resize(|r| r.fixed_size(egui::vec2(600.0, 700.0))).show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("📂 Mở file Text").clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("Text", &["txt"]).pick_file() {
                         if let Ok(content) = std::fs::read_to_string(path) {
                             self.ui_state.reader.raw_text = content;
                             self.ui_state.reader.parse_text();
                         }
                    }
                }
                if ui.button("🧹 Xóa hết").clicked() {
                    self.ui_state.reader.raw_text.clear();
                    self.ui_state.reader.chunks.clear();
                    self.ui_state.reader.is_playing = false;
                    self.ui_state.reader.current_index = 0;
                }
            });
            ui.label("Nhập văn bản vào đây:");
            if ui.add(egui::TextEdit::multiline(&mut self.ui_state.reader.raw_text).desired_rows(5).desired_width(f32::INFINITY)).changed() {
                self.ui_state.reader.is_playing = false;
                self.ui_state.reader.parse_text();
            }
            ui.separator();
            ui.horizontal(|ui| {
                let icon_play = if self.ui_state.reader.is_playing { "⏸ Tạm dừng" } else { "▶ Đọc tiếp" };
                if ui.button(icon_play).clicked() {
                     self.ui_state.reader.is_playing = !self.ui_state.reader.is_playing;
                     if self.ui_state.reader.chunks.is_empty() { self.ui_state.reader.parse_text(); self.ui_state.reader.is_playing = true; }
                }
                if ui.button("⏹ Dừng lại").clicked() { self.ui_state.reader.is_playing = false; self.ui_state.reader.current_index = 0; }
                ui.label("Tốc độ:");
                ui.add(egui::Slider::new(&mut self.config_state.config.speed, 0.5..=2.0));
            });
            ui.separator();
            ui.label(egui::RichText::new("Danh sách câu (Nhấn vào để đọc từ câu đó):").strong());
            egui::ScrollArea::vertical().stick_to_bottom(true).max_height(400.0).show(ui, |ui| {
                for (i, chunk) in self.ui_state.reader.chunks.iter().enumerate() {
                    let is_active = i == self.ui_state.reader.current_index;
                    let text = format!("{}. {}", i + 1, chunk);
                    let label = egui::SelectableLabel::new(is_active, text);
                    if ui.add_sized([ui.available_width(), 0.0], label).clicked() {
                        self.ui_state.reader.current_index = i;
                        self.ui_state.reader.is_playing = true;
                        self.next_audio_buffer = None;
                        self.is_downloading_next = false;
                    }
                }
            });
        });
        self.ui_state.reader.is_open = open;
    }

    fn load_texture(&mut self, ctx: &egui::Context, bytes: &[u8], is_arrow: bool) {
        if let Ok(image) = image::load_from_memory(bytes) {
            let size = [image.width() as usize, image.height() as usize];
            let image_buffer = image.to_rgba8();
            let pixels = image_buffer.into_raw();
            let image_data = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
            let name = if is_arrow { "arrow_img" } else { "area_img" };
            let texture = ctx.load_texture(name, image_data, egui::TextureOptions::default());
            if is_arrow { self.arrow_texture = Some(texture); }
        }
    }
}