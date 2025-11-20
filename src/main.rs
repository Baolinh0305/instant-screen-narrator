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
use image;

// Các biến toàn cục để quản lý trạng thái giữa UI và luồng ngầm
static LAST_SELECT: AtomicU64 = AtomicU64::new(0);
static OVERLAY_ACTIVE: AtomicBool = AtomicBool::new(false);
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
    started: bool,
    listener_spawned: bool,
    show_popup: bool,
    popup_text: String,
    in_custom_mode: bool,
    use_tts: bool,
    show_image_window: bool,
    image_texture: Option<egui::TextureHandle>,
    // Biến đếm thời gian để hiển thị thông báo "Đã chọn vùng"
    wwm_success_timer: Option<std::time::Instant>,
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
            listener_spawned: false,
            show_popup: false,
            popup_text: String::new(),
            in_custom_mode: false,
            use_tts: config.use_tts,
            show_image_window: false,
            image_texture: None,
            wwm_success_timer: None,
        }
    }
}

impl MainApp {
    fn configure_style(&self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.spacing.window_margin = egui::Margin::same(15.0);
        style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(5.0);
        style.visuals.widgets.inactive.rounding = egui::Rounding::same(5.0);
        style.visuals.widgets.hovered.rounding = egui::Rounding::same(5.0);
        style.visuals.widgets.active.rounding = egui::Rounding::same(5.0);

        let border_color = egui::Color32::from_rgb(100, 149, 237);
        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, border_color);
        style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.5, border_color);
        style.visuals.widgets.active.bg_stroke = egui::Stroke::new(2.0, border_color);
        ctx.set_style(style);
    }

    fn start_service(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel::<(String, bool, f32, bool)>();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            while let Ok((text, split_tts, speed, use_tts)) = rx.recv() {
                rt.block_on(async {
                    if let Err(_e) = tts::speak(&text, split_tts, speed, use_tts).await {}
                });
            }
        });

        std::thread::spawn(move || {
            let tx_clone = tx.clone();
            let callback = move |event: Event| {
                if LISTENING_PAUSED.load(Ordering::Relaxed) { return; }
                let config = config::Config::load();
                if let EventType::KeyPress(_) = event.event_type {
                    if event.name.as_ref() == Some(&config.hotkey_translate) {
                        let tx = tx_clone.clone();
                        std::thread::spawn(move || {
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
                                                Ok(translated) => text.push_str(&translated),
                                                Err(_) => text.push_str("(Translation error)"),
                                            }
                                        } else { text.push_str("(No API key set)"); }
                                        text.push(' ');
                                    }
                                }
                                if !text.is_empty() {
                                    let _ = tx.send((text.clone(), split_tts, config.speed, config.use_tts));
                                    if config.show_overlay {
                                        if let Some(region) = regions.first() {
                                            let rect = RECT { left: region.x, top: region.y, right: region.x + region.width as i32, bottom: region.y + region.height as i32 };
                                            let duration_ms = (text.chars().count() as f32 / 10.0 * 1000.0) as u32;
                                            std::thread::spawn(move || { show_result_window(rect, text.clone(), duration_ms); });
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
                                std::thread::spawn(|| { overlay::show_selection_overlay(); OVERLAY_ACTIVE.store(false, Ordering::Relaxed); });
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
                                                           let rect = RECT { left: region.x, top: region.y, right: region.x + region.width as i32, bottom: region.y + region.height as i32 };
                                                           let duration_ms = (translated.chars().count() as f32 / 10.0 * 1000.0) as u32;
                                                           std::thread::spawn(move || { show_result_window(rect, translated.clone(), duration_ms); });
                                                       }
                                                   }
                                                   Err(_) => println!("Translation error"),
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
            if let Err(_error) = listen(callback) {}
        });
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

        self.configure_style(ctx);

        if self.image_texture.is_none() {
            let image_bytes = include_bytes!("area.png"); 
            if let Ok(image) = image::load_from_memory(image_bytes) {
                let size = [image.width() as usize, image.height() as usize];
                let image_buffer = image.to_rgba8();
                let pixels = image_buffer.into_raw();
                let image_data = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
                self.image_texture = Some(ctx.load_texture("area_image", image_data, egui::TextureOptions::default()));
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading(egui::RichText::new("Screen Translator").strong().size(24.0));
                ui.add_space(10.0);
            });

            egui::ScrollArea::vertical().show(ui, |ui| {
                // KHỐI 1: API
                egui::CollapsingHeader::new(egui::RichText::new("🌐 Cấu hình API").strong()).default_open(true).show(ui, |ui| {
                    egui::Grid::new("api_grid").num_columns(2).spacing([20.0, 15.0]).striped(true).show(ui, |ui| {
                        ui.label("Dịch vụ:");
                        ui.horizontal(|ui| {
                            ui.add_enabled_ui(!self.started, |ui| {
                                egui::ComboBox::from_id_source("api_selector")
                                    .selected_text(if self.selected_api == "gemini" { "Gemini (Không nên dùng)" } else { "Groq (Nhanh)" })
                                    .width(200.0)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut self.selected_api, "gemini".to_string(), "Gemini (Không nên dùng)");
                                        ui.selectable_value(&mut self.selected_api, "groq".to_string(), "Groq (Siêu nhanh)");
                                    });
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add(egui::Button::new(egui::RichText::new("❓ Hướng dẫn lấy Key").small())).clicked() {
                                    self.show_popup = true;
                                    self.popup_text = if self.selected_api == "gemini" { "gemini".to_string() } else { "groq".to_string() };
                                }
                            });
                        });
                        ui.end_row();
                        ui.label("API Key:");
                        let key_ref = if self.selected_api == "gemini" { &mut self.gemini_api_key } else { &mut self.groq_api_key };
                        ui.add_enabled(!self.started, egui::TextEdit::singleline(key_ref).password(true).desired_width(f32::INFINITY));
                        ui.end_row();
                    });
                });
                ui.add_space(5.0);

                // KHỐI 2: PROMPT
                egui::CollapsingHeader::new(egui::RichText::new("📝 Cấu hình Dịch (Prompt)").strong()).default_open(true).show(ui, |ui| {
                    ui.add_enabled_ui(!self.started, |ui| {
                        ui.horizontal(|ui| {
                            if ui.button("🗡️ Kiếm hiệp").clicked() { self.current_prompt = config::Config::get_wuxia_prompt(); self.in_custom_mode = false; }
                            if ui.button("🌍 Thông thường").clicked() { self.current_prompt = config::Config::get_normal_prompt(); self.in_custom_mode = false; }
                            if ui.button("🔧 Custom").clicked() { self.current_prompt = self.custom_prompt.clone(); self.in_custom_mode = true; }
                        });
                    });
                    ui.add_enabled(!self.started, egui::TextEdit::multiline(&mut self.current_prompt).desired_rows(4).desired_width(f32::INFINITY));
                    if self.in_custom_mode {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                            if ui.add_enabled(!self.started, egui::Button::new("💾 Lưu Custom Prompt")).clicked() {
                                self.custom_prompt = self.current_prompt.clone();
                                self.config.custom_prompt = self.custom_prompt.clone();
                                self.config.save().unwrap();
                            }
                        });
                    }
                });
                ui.add_space(5.0);

                // KHỐI 3: PHÍM TẮT
                egui::CollapsingHeader::new(egui::RichText::new("⌨️ Phím tắt (hiện tại chỉ dùng được phím đơn)").strong()).default_open(true).show(ui, |ui| {
                     egui::Grid::new("hotkey_grid").num_columns(2).spacing([20.0, 10.0]).striped(true).show(ui, |ui| {
                        ui.label("Dịch vùng đã chọn:"); ui.add_enabled(!self.started, egui::TextEdit::singleline(&mut self.hotkey_translate).desired_width(80.0)); ui.end_row();
                        ui.label("Chọn vùng mới:"); ui.add_enabled(!self.started, egui::TextEdit::singleline(&mut self.hotkey_select).desired_width(80.0)); ui.end_row();
                        ui.label("Chụp & Dịch ngay:"); ui.add_enabled(!self.started, egui::TextEdit::singleline(&mut self.hotkey_instant).desired_width(80.0)); ui.end_row();
                    });
                });
                ui.add_space(5.0);

                // KHỐI 4: CÀI ĐẶT
                egui::CollapsingHeader::new(egui::RichText::new("⚙️ Cài đặt hiển thị & Âm thanh").strong()).default_open(true).show(ui, |ui| {
                    egui::Grid::new("settings_grid").num_columns(2).spacing([20.0, 10.0]).show(ui, |ui| {
                        ui.label("Overlay:"); ui.add_enabled(!self.started, egui::Checkbox::new(&mut self.config.show_overlay, "Hiện văn bản trên màn hình")); ui.end_row();
                        ui.label("TTS (Đọc):");
                        ui.horizontal(|ui| {
                            ui.add_enabled(!self.started, egui::Checkbox::new(&mut self.use_tts, "Bật đọc giọng nói"));
                            if self.use_tts { ui.add_enabled(!self.started, egui::Checkbox::new(&mut self.config.split_tts, "Split TTS (Chia nhỏ câu)")); }
                        });
                        ui.end_row();
                        ui.label("Tốc độ đọc:"); ui.add_enabled(!self.started, egui::Slider::new(&mut self.config.speed, 0.5..=2.0).text("x")); ui.end_row();
                    });
                });
                ui.add_space(20.0);

                // LOGIC NÚT ACTION
                ui.vertical_centered(|ui| {
                    // === HÀNG NÚT CHỨC NĂNG PHỤ ===
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Center), |ui| {
                            
                            // --- XỬ LÝ TRẠNG THÁI NÚT WWM ---
                            let mut wwm_text = "🎯 Tự động chọn vùng WWM";
                            if let Some(time) = self.wwm_success_timer {
                                if time.elapsed().as_secs_f32() < 1.0 {
                                    wwm_text = "✅ Đã chọn vùng";
                                    ctx.request_repaint(); // Vẽ lại liên tục để update text sau 1s
                                } else {
                                    self.wwm_success_timer = None; // Reset timer
                                }
                            }

                            // --- NÚT WWM (Chỉ bật khi started = true) ---
                            // Sử dụng ui.add_enabled để disable khi chưa Start
                            if ui.add_enabled(self.started, egui::Button::new(wwm_text)).clicked() {
                                let region = config::Region { x: 308, y: 919, width: 1313, height: 135 };
                                self.config.fixed_regions.clear();
                                self.config.fixed_regions.push(region);
                                self.config.save().unwrap();
                                
                                // Kích hoạt timer đổi text
                                self.wwm_success_timer = Some(std::time::Instant::now());
                            }

                            ui.label(egui::RichText::new("(Dành cho màn 16:9 như ảnh)").italics().color(egui::Color32::GRAY));
                            if ui.button("🖼️ Ảnh").clicked() { self.show_image_window = true; }
                        });
                    });
                    ui.add_space(10.0);
                    // =========================================

                    if !self.started {
                        let start_btn = egui::Button::new(egui::RichText::new("🚀 BẮT ĐẦU SỬ DỤNG").size(20.0).strong().color(egui::Color32::WHITE))
                            .min_size(egui::vec2(200.0, 50.0)).fill(egui::Color32::from_rgb(0, 120, 215)); 
                        if ui.add(start_btn).clicked() {
                            self.config.gemini_api_key = self.gemini_api_key.clone();
                            self.config.groq_api_key = self.groq_api_key.clone();
                            self.config.current_prompt = self.current_prompt.clone();
                            self.config.hotkey_translate = self.hotkey_translate.clone();
                            self.config.hotkey_select = self.hotkey_select.clone();
                            self.config.hotkey_instant = self.hotkey_instant.clone();
                            self.config.selected_api = self.selected_api.clone();
                            self.config.use_tts = self.use_tts;
                            self.config.save().unwrap();
                            self.started = true;
                            LISTENING_PAUSED.store(false, Ordering::Relaxed);
                            if !self.listener_spawned { self.start_service(); self.listener_spawned = true; }
                        }
                    } else {
                        let stop_btn = egui::Button::new(egui::RichText::new("⏹ Dừng").size(20.0).strong().color(egui::Color32::WHITE))
                            .min_size(egui::vec2(250.0, 50.0)).fill(egui::Color32::from_rgb(200, 50, 50));
                        if ui.add(stop_btn).clicked() { self.started = false; LISTENING_PAUSED.store(true, Ordering::Relaxed); }
                    }
                    ui.add_space(10.0);
                    ui.colored_label(egui::Color32::GRAY, "ℹ Chạy Admin nếu muốn dịch game Fullscreen");
                });
            });
        });

        // POPUP API
        if self.show_popup {
            let mut open = true;
            egui::Window::new("Hướng dẫn lấy API Key").collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
                if self.popup_text == "gemini" {
                    ui.heading("Gemini API");
                    ui.horizontal(|ui| { ui.label("1. Vào:"); ui.hyperlink("https://aistudio.google.com/api-keys"); });
                    ui.label("2. Đăng nhập Google -> Create API key"); ui.label("3. Copy key và dán vào tool");
                } else if self.popup_text == "groq" {
                    ui.heading("Groq API (Nhanh)");
                    ui.horizontal(|ui| { ui.label("1. Vào:"); ui.hyperlink("https://console.groq.com/keys"); });
                    ui.label("2. Đăng nhập -> Create API Key"); ui.label("3. Copy key và dán vào tool");
                }
                ui.separator();
                ui.vertical_centered(|ui| { if ui.button("Đã hiểu").clicked() { self.show_popup = false; } });
            });
            if !open { self.show_popup = false; }
        }

        // CỬA SỔ ẢNH
        if self.show_image_window {
            let mut open = true;
            egui::Window::new("Ảnh Minh Họa (16:9)").open(&mut open).collapsible(false).resizable(true).default_size(egui::vec2(800.0, 500.0)).show(ctx, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    if ui.button("❌ Đóng").clicked() { self.show_image_window = false; }
                });
                ui.separator();
                if let Some(texture) = &self.image_texture {
                    let available_size = ui.available_size();
                    let original_size = texture.size_vec2();
                    let w_ratio = available_size.x / original_size.x;
                    let h_ratio = available_size.y / original_size.y;
                    let scale = w_ratio.min(h_ratio); 
                    let fit_size = original_size * scale;
                    ui.centered_and_justified(|ui| { ui.image((texture.id(), fit_size)); });
                } else {
                    ui.label("Đang tải ảnh hoặc không tìm thấy 'area.png'...");
                }
            });
            if !open { self.show_image_window = false; }
        }
    }
}

fn main() -> Result<(), eframe::Error> {
    let mut options = eframe::NativeOptions::default();
    options.viewport.transparent = Some(false);
    options.viewport.inner_size = Some(egui::vec2(900.0, 900.0));
    options.viewport.taskbar = Some(true);
    eframe::run_native(
        "Screen Translator",
        options,
        Box::new(|_cc| Box::new(MainApp::default())),
    )
}