#![windows_subsystem = "windows"]

mod config;
mod capture;
mod translation;
mod tts;
mod overlay;

use crate::overlay::show_result_window;
use eframe::egui;

use rdev::{listen, Event, EventType};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use winapi::shared::windef::RECT;

static LAST_SELECT: AtomicU64 = AtomicU64::new(0);
static OVERLAY_ACTIVE: AtomicBool = AtomicBool::new(false);
// Thêm biến này để kiểm soát trạng thái tạm dừng/tiếp tục
static LISTENING_PAUSED: AtomicBool = AtomicBool::new(true); 

struct MainApp {
    config: config::Config,
    gemini_api_key: String,
    groq_api_key: String,
    current_prompt: String,
    custom_prompt: String,
    hotkey_translate: String,
    hotkey_select: String,
    hotkey_instant: String,
    selected_api: String,
    started: bool, // Trạng thái UI (Lock/Unlock)
    listener_spawned: bool, // Kiểm tra xem service đã chạy lần đầu chưa
    show_popup: bool,
    popup_text: String,
    in_custom_mode: bool,
    use_tts: bool,
}

impl Default for MainApp {
    fn default() -> Self {
        let config = config::Config::load();
        Self {
            config: config.clone(),
            gemini_api_key: config.gemini_api_key,
            groq_api_key: config.groq_api_key,
            current_prompt: config.current_prompt,
            custom_prompt: config.custom_prompt,
            hotkey_translate: config.hotkey_translate,
            hotkey_select: config.hotkey_select,
            hotkey_instant: config.hotkey_instant,
            selected_api: config.selected_api,
            started: false,
            listener_spawned: false, // Mặc định chưa chạy service
            show_popup: false,
            popup_text: String::new(),
            in_custom_mode: false,
            use_tts: config.use_tts,
        }
    }
}

impl eframe::App for MainApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert("roboto".to_owned(), egui::FontData::from_static(include_bytes!("roboto.ttf")));
        fonts.families.get_mut(&egui::FontFamily::Proportional).unwrap().insert(0, "roboto".to_owned());
        fonts.families.get_mut(&egui::FontFamily::Monospace).unwrap().push("roboto".to_owned());
        ctx.set_fonts(fonts);
        ctx.set_pixels_per_point(1.2);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Screen Translator");
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label("Chọn API để dịch:");
                // Vô hiệu hóa UI khi started = true
                ui.set_enabled(!self.started); 
                ui.horizontal(|ui| {
                    egui::ComboBox::from_label("")
                        .selected_text(if self.selected_api == "gemini" { "Gemini" } else { "Groq" })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.selected_api, "gemini".to_string(), "Gemini");
                            ui.selectable_value(&mut self.selected_api, "groq".to_string(), "Groq");
                        });
                    ui.label(if self.selected_api == "gemini" { "Chậm, để cho vui không nên chọn" } else { "Nhanh, mỗi api key dịch được 1000 lần/ngày, hết thì đổi api key" });
                });
                ui.set_enabled(true); // Bật lại để nút popup vẫn bấm được nếu cần (tùy chọn)
                ui.horizontal(|ui| {
                    if self.selected_api == "groq" {
                        if ui.button("Cách lấy groq api key").clicked() {
                            self.show_popup = true;
                            self.popup_text = "groq".to_string();
                        }
                    } else {
                        if ui.button("Cách lấy gemini api key").clicked() {
                            self.show_popup = true;
                            self.popup_text = "gemini".to_string();
                        }
                    }
                });
                // Group lại để set enabled chung
                ui.scope(|ui| {
                    ui.set_enabled(!self.started);
                    if self.selected_api == "gemini" {
                        ui.horizontal(|ui| {
                            ui.label("Nhập Gemini api key:");
                            ui.add(egui::TextEdit::singleline(&mut self.gemini_api_key).password(true));
                        });
                    } else {
                        ui.horizontal(|ui| {
                            ui.label("Nhập Groq api key:");
                            ui.add(egui::TextEdit::singleline(&mut self.groq_api_key).password(true));
                        });
                    }
                });
            });

            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_enabled(!self.started);
                ui.label("Prompt Dịch:");
                ui.horizontal(|ui| {
                    if ui.button("Dịch kiểu kiếm hiệp").clicked() {
                        self.current_prompt = config::Config::get_wuxia_prompt();
                        self.in_custom_mode = false;
                    }
                    if ui.button("Dịch kiểu bình thường").clicked() {
                        self.current_prompt = config::Config::get_normal_prompt();
                        self.in_custom_mode = false;
                    }
                    if ui.button("Custom").clicked() {
                        self.current_prompt = self.custom_prompt.clone();
                        self.in_custom_mode = true;
                    }
                });
                ui.add(egui::TextEdit::multiline(&mut self.current_prompt).desired_rows(5));
                if self.in_custom_mode && ui.button("Lưu").clicked() {
                    self.custom_prompt = self.current_prompt.clone();
                    self.config.custom_prompt = self.custom_prompt.clone();
                    self.config.save().unwrap();
                }
            });
            ui.separator();
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_enabled(!self.started);
                ui.label("Phím tắt: (hiện chỉ hỗ trợ phím đơn)");
                ui.horizontal(|ui| {
                    ui.label("Nút dịch:");
                    ui.add(egui::TextEdit::singleline(&mut self.hotkey_translate).desired_width(50.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Nút chọn vùng cần dịch:");
                    ui.add(egui::TextEdit::singleline(&mut self.hotkey_select).desired_width(50.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Nút chụp nhanh:");
                    ui.add(egui::TextEdit::singleline(&mut self.hotkey_instant).desired_width(50.0));
                });
            });
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_enabled(!self.started);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.use_tts, "Use TTS");
                    ui.label("Đọc văn bản");
                });
                ui.horizontal(|ui| {
                    ui.add_enabled(self.use_tts, egui::Checkbox::new(&mut self.config.split_tts, "Split TTS"));
                    ui.label("Bắt buộc chọn, chia đoạn văn ra thành từng phần nhỏ để chuyển thành giọng nói rồi ghép lại");
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.config.show_overlay, "Hiển thị văn bản dịch trên vùng dịch");
                });
                ui.add(egui::Slider::new(&mut self.config.speed, 0.0..=2.0).text("Tốc độ đọc"));
            });

            // LOGIC NÚT BẤM START/STOP Ở ĐÂY
            ui.add_space(10.0);
            if !self.started {
                // Trạng thái chưa bắt đầu hoặc đang tạm dừng
                if ui.button("Bắt đầu").clicked() {
                    // 1. Lưu config
                    self.config.gemini_api_key = self.gemini_api_key.clone();
                    self.config.groq_api_key = self.groq_api_key.clone();
                    self.config.current_prompt = self.current_prompt.clone();
                    self.config.hotkey_translate = self.hotkey_translate.clone();
                    self.config.hotkey_select = self.hotkey_select.clone();
                    self.config.hotkey_instant = self.hotkey_instant.clone();
                    self.config.selected_api = self.selected_api.clone();
                    self.config.use_tts = self.use_tts;
                    self.config.save().unwrap();

                    // 2. Lock UI
                    self.started = true;

                    // 3. Cho phép lắng nghe sự kiện
                    LISTENING_PAUSED.store(false, Ordering::Relaxed);

                    // 4. Chỉ spawn thread lần đầu tiên
                    if !self.listener_spawned {
                        self.start_service();
                        self.listener_spawned = true;
                    }
                }
            } else {
                // Trạng thái đang chạy -> Hiển thị nút Dừng/Sửa
                if ui.button("Mở khóa để chỉnh sửa").clicked() {
                    // 1. Unlock UI
                    self.started = false;
                    
                    // 2. Tạm dừng lắng nghe sự kiện (nhưng không kill thread)
                    LISTENING_PAUSED.store(true, Ordering::Relaxed);
                }
            }
            
            ui.colored_label(egui::Color32::RED, "Lưu ý: Muốn dịch game hay app fullscreen thì phải chạy app bằng quyền quản trị");
        });

        if self.show_popup {
            let mut open = true;
            egui::Window::new("Hướng dẫn")
                .open(&mut open)
                .show(ctx, |ui| {
                    if self.popup_text == "gemini" {
                        ui.label("Để lấy Gemini API key:");
                        ui.hyperlink_to("1. Vào https://aistudio.google.com/api-keys", "https://aistudio.google.com/api-keys");
                        ui.label("...");
                    } else if self.popup_text == "groq" {
                        ui.label("Để lấy Groq API key:");
                        ui.hyperlink_to("1. Vào https://console.groq.com/keys", "https://console.groq.com/keys");
                        ui.label("...");
                    }
                    if ui.button("Đóng").clicked() {
                        self.show_popup = false;
                    }
                });
            if !open {
                self.show_popup = false;
            }
        }
    }
}


impl MainApp {
    fn start_service(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel::<(String, bool, f32, bool)>();
        
        // Thread xử lý TTS (không cần thay đổi nhiều)
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            while let Ok((text, split_tts, speed, use_tts)) = rx.recv() {
                rt.block_on(async {
                    if let Err(_e) = tts::speak(&text, split_tts, speed, use_tts).await {
                    }
                });
            }
        });

        // Thread lắng nghe phím
        std::thread::spawn(move || {
            let tx_clone = tx.clone();
            let callback = move |event: Event| {
                // QUAN TRỌNG: Kiểm tra xem có đang bị tạm dừng không
                if LISTENING_PAUSED.load(Ordering::Relaxed) {
                    return;
                }

                // Mỗi lần nhấn phím sẽ load lại config từ file -> Đảm bảo cập nhật thay đổi sau khi chỉnh sửa
                let config = config::Config::load();
                
                if let EventType::KeyPress(_) = event.event_type {
                    if event.name.as_ref() == Some(&config.hotkey_translate) {
                        let tx = tx_clone.clone();
                        std::thread::spawn(move || {
                            // Code xử lý dịch (giữ nguyên logic cũ)
                            let config = config::Config::load();
                            let selected_api = config.selected_api.clone();
                            let gemini_key = config.gemini_api_key.clone();
                            let groq_key = config.groq_api_key.clone();
                            let prompt = config.current_prompt;
                            let regions = config.fixed_regions.clone();
                            let split_tts = config.split_tts;
                            
                            let rt = tokio::runtime::Runtime::new().unwrap();
                            rt.block_on(async {
                                let mut text = String::new();
                                for region in &regions {
                                    let image_bytes = capture::capture_image(region).unwrap_or_default();
                                    if !image_bytes.is_empty() {
                                        let api_key = if selected_api == "gemini" { &gemini_key } else { &groq_key };
                                        if !api_key.is_empty() {
                                            match translation::translate_from_image(&selected_api, api_key, &prompt, &image_bytes).await {
                                                Ok(translated) => {
                                                    text.push_str(&translated);
                                                }
                                                Err(_) => {
                                                    text.push_str("(Translation error)");
                                                }
                                            }
                                        } else {
                                            text.push_str("(No API key set)");
                                        }
                                        text.push(' ');
                                    }
                                }
                                if !text.is_empty() {
                                    let _ = tx.send((text.clone(), split_tts, config.speed, config.use_tts));
                                    if config.show_overlay {
                                        if let Some(region) = regions.first() {
                                            let rect = RECT {
                                                left: region.x,
                                                top: region.y,
                                                right: region.x + region.width as i32,
                                                bottom: region.y + region.height as i32,
                                            };
                                            let char_count = text.chars().count();
                                            let duration_sec = char_count as f32 / 10.0;
                                            let duration_ms = (duration_sec * 1000.0) as u32;
                                            std::thread::spawn(move || {
                                                show_result_window(rect, text.clone(), duration_ms);
                                            });
                                        }
                                    }
                                }
                            });
                        });
                    } else if event.name.as_ref() == Some(&config.hotkey_select) {
                        if !OVERLAY_ACTIVE.load(Ordering::Relaxed) {
                            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
                            if now - LAST_SELECT.load(Ordering::Relaxed) > 1000 {
                                LAST_SELECT.store(now, Ordering::Relaxed);
                                OVERLAY_ACTIVE.store(true, Ordering::Relaxed);
                                std::thread::spawn(|| {
                                    overlay::show_selection_overlay();
                                    OVERLAY_ACTIVE.store(false, Ordering::Relaxed);
                                });
                            }
                        }
                    } else if event.name.as_ref() == Some(&config.hotkey_instant) {
                       if !OVERLAY_ACTIVE.load(Ordering::Relaxed) {
                           OVERLAY_ACTIVE.store(true, Ordering::Relaxed);
                           let tx_for_thread = tx_clone.clone();
                           std::thread::spawn(move || {
                               overlay::show_selection_overlay();
                               OVERLAY_ACTIVE.store(false, Ordering::Relaxed);
                               let config = config::Config::load();
                               if let Some(region) = config.fixed_regions.last() {
                                   let image_bytes = capture::capture_image(region).unwrap_or_default();
                                   if !image_bytes.is_empty() {
                                       let api_key = if config.selected_api == "gemini" { &config.gemini_api_key } else { &config.groq_api_key };
                                       if !api_key.is_empty() {
                                           let rt = tokio::runtime::Runtime::new().unwrap();
                                           rt.block_on(async {
                                               match translation::translate_from_image(&config.selected_api, api_key, &config.current_prompt, &image_bytes).await {
                                                   Ok(translated) => {
                                                       let tx = tx_for_thread.clone();
                                                       let _ = tx.send((translated.clone(), config.split_tts, config.speed, config.use_tts));
                                                       if config.show_overlay {
                                                           let rect = RECT {
                                                               left: region.x,
                                                               top: region.y,
                                                               right: region.x + region.width as i32,
                                                               bottom: region.y + region.height as i32,
                                                           };
                                                           let char_count = translated.chars().count();
                                                           let duration_sec = char_count as f32 / 10.0;
                                                           let duration_ms = (duration_sec * 1000.0) as u32;
                                                           std::thread::spawn(move || {
                                                               show_result_window(rect, translated.clone(), duration_ms);
                                                           });
                                                       }
                                                   }
                                                   Err(_) => {
                                                       println!("Translation error");
                                                   }
                                               }
                                           });
                                       }
                                   }
                               }
                           });
                       }
                   }
                }
            };
            if let Err(_error) = listen(callback) {
            }
        });
    }
}

fn main() -> Result<(), eframe::Error> {
    let mut options = eframe::NativeOptions::default();
    options.viewport.transparent = Some(false);
    options.viewport.inner_size = Some(egui::vec2(800.0, 800.0));
    options.viewport.taskbar = Some(true);
    eframe::run_native(
        "Screen Translator",
        options,
        Box::new(|_cc| Box::new(MainApp::default())),
    )
}