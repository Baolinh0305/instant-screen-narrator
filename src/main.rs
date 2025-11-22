#![windows_subsystem = "windows"] 

mod config;
mod capture;
mod translation;
mod tts;
mod overlay;

use crate::overlay::show_result_window;
use eframe::egui;
use rdev::{listen, Event, EventType};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicI32, Ordering}; 
use std::sync::mpsc::Sender;
use winapi::shared::windef::RECT;
use winapi::um::winuser::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
use image;
use std::fs;

const DEFAULT_ARROW: &[u8] = include_bytes!("arrow.png");
const AREA_PLACEHOLDER: &[u8] = include_bytes!("area.png");

static LAST_SELECT: AtomicU64 = AtomicU64::new(0);
static OVERLAY_ACTIVE: AtomicBool = AtomicBool::new(false);
static LISTENING_PAUSED: AtomicBool = AtomicBool::new(true);
static AUTO_TRANSLATE_ENABLED: AtomicBool = AtomicBool::new(false);
static GROQ_REMAINING: AtomicI32 = AtomicI32::new(-1); 

struct MainApp {
    config: config::Config,
    gemini_api_key: String,
    current_prompt: String,
    custom_prompt: String,
    hotkey_translate: String,
    hotkey_select: String,
    hotkey_instant: String,
    hotkey_auto: String,
    selected_api: String,
    started: bool,
    listener_spawned: bool,
    show_popup: bool,
    popup_text: String,
    in_custom_mode: bool,
    use_tts: bool,
    show_image_window: bool,
    show_arrow_window: bool,
    image_texture: Option<egui::TextureHandle>,
    arrow_texture: Option<egui::TextureHandle>,
    wwm_success_timer: Option<std::time::Instant>,
    arrow_wwm_success_timer: Option<std::time::Instant>,
    auto_translate_active: bool,
    show_password: bool,
}

impl Default for MainApp {
    fn default() -> Self {
        let mut config = config::Config::load();
        if config.groq_api_keys.is_empty() {
            config.groq_api_keys.push(String::new());
        }

        Self {
            config: config.clone(),
            gemini_api_key: config.gemini_api_key,
            current_prompt: config.current_prompt,
            custom_prompt: config.custom_prompt,
            hotkey_translate: config.hotkey_translate,
            hotkey_select: config.hotkey_select,
            hotkey_instant: config.hotkey_instant,
            hotkey_auto: config.hotkey_auto,
            selected_api: config.selected_api,
            started: false,
            listener_spawned: false,
            show_popup: false,
            popup_text: String::new(),
            in_custom_mode: false,
            use_tts: config.use_tts,
            show_image_window: false,
            show_arrow_window: false,
            image_texture: None,
            arrow_texture: None,
            wwm_success_timer: None,
            arrow_wwm_success_timer: None,
            auto_translate_active: false,
            show_password: false,
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

    async fn perform_translation_logic(
        config: config::Config,
        tx: Sender<(String, bool, f32, bool)>
    ) {
        let regions = config.fixed_regions.clone();
        Self::translate_regions(config, regions, tx).await;
    }

    async fn perform_instant_translation(
        config: config::Config,
        tx: Sender<(String, bool, f32, bool)>,
        region: config::Region
    ) {
        Self::translate_regions(config, vec![region], tx).await;
    }

    async fn translate_regions(
        mut config: config::Config,
        regions: Vec<config::Region>,
        tx: Sender<(String, bool, f32, bool)>
    ) {
        let prompt = config.current_prompt.clone();
        let mut final_text = String::new();
        
        for region in &regions {
            let image_bytes = capture::capture_image(region).unwrap_or_default();
            if !image_bytes.is_empty() {
                let mut attempts = 0;
                let max_attempts = if config.selected_api == "groq" { config.groq_api_keys.len() } else { 1 };
                let mut success = false;

                while attempts < max_attempts {
                    let api_key = if config.selected_api == "gemini" {
                        config.gemini_api_key.clone()
                    } else {
                        config.get_current_groq_key()
                    };

                    if api_key.is_empty() {
                        final_text.push_str("(Chưa nhập Key) ");
                        break;
                    }

                    match translation::translate_from_image(&config.selected_api, &api_key, &prompt, &image_bytes).await {
                        Ok(result) => {
                            final_text.push_str(&result.text);
                            if let Some(rem) = result.remaining_requests {
                                GROQ_REMAINING.store(rem, Ordering::Relaxed);
                            }
                            success = true;
                            break; 
                        },
                        Err(translation::TranslationError::RateLimitExceeded) => {
                            if config.selected_api == "groq" && config.groq_api_keys.len() > 1 {
                                println!("Key {} hết hạn mức. Đang đổi key...", config.active_groq_index);
                                config.active_groq_index = (config.active_groq_index + 1) % config.groq_api_keys.len();
                                attempts += 1;
                                continue;
                            } else {
                                final_text.push_str("(Hết lượt Request & hết Key dự phòng) ");
                                break;
                            }
                        },
                        Err(translation::TranslationError::Other(e)) => {
                            let error_msg = format!("Lỗi: {} ", e);
                            println!("{}", error_msg);
                            final_text.push_str(&error_msg);
                            break; 
                        }
                    }
                }
                if !success && final_text.is_empty() { final_text.push_str("... "); }
                final_text.push(' ');
            }
        }

        if !final_text.trim().is_empty() {
            let _ = tx.send((final_text.clone(), config.split_tts, config.speed, config.use_tts));
            if config.show_overlay {
                if let Some(region) = regions.first() {
                    let rect = RECT { left: region.x, top: region.y, right: region.x + region.width as i32, bottom: region.y + region.height as i32 };
                    let duration_ms = (final_text.chars().count() as f32 / 10.0 * 1000.0) as u32;
                    std::thread::spawn(move || { show_result_window(rect, final_text.clone(), duration_ms); });
                }
            }
        }
    }

    fn load_texture(&mut self, ctx: &egui::Context, bytes: &[u8], is_arrow: bool) {
        if let Ok(image) = image::load_from_memory(bytes) {
            let size = [image.width() as usize, image.height() as usize];
            let image_buffer = image.to_rgba8();
            let pixels = image_buffer.into_raw();
            let image_data = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
            let name = if is_arrow { "arrow_img" } else { "area_img" };
            let texture = ctx.load_texture(name, image_data, egui::TextureOptions::default());
            if is_arrow { self.arrow_texture = Some(texture); } else { self.image_texture = Some(texture); }
        }
    }

    fn start_service(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel::<(String, bool, f32, bool)>();
        
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            while let Ok((text, split_tts, speed, use_tts)) = rx.recv() {
                rt.block_on(async { if let Err(_e) = tts::speak(&text, split_tts, speed, use_tts).await {} });
            }
        });

        let tx_clone = tx.clone();
        let tx_auto = tx.clone();

        // Thread Auto Translate
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            
            let load_arrow = || -> Vec<u8> {
                let custom_path = config::Config::get_custom_arrow_path();
                if custom_path.exists() { if let Ok(b) = fs::read(custom_path) { return b; } }
                DEFAULT_ARROW.to_vec()
            };

            let arrow_bytes = load_arrow();
            let mut last_found_state = false;
            let mut miss_counter = 0;
            const MISS_TOLERANCE: i32 = 5;

            loop {
                let enabled = AUTO_TRANSLATE_ENABLED.load(Ordering::Relaxed);
                if !enabled || arrow_bytes.is_empty() {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    continue;
                }

                let config = config::Config::load();
                if let Some(arrow_region) = &config.arrow_region {
                    let found = capture::is_template_present(arrow_region, &arrow_bytes);
                    if found {
                        miss_counter = 0; 
                        if !last_found_state {
                            let tx_inner = tx_auto.clone();
                            let config_clone = config.clone();
                            rt.block_on(async { Self::perform_translation_logic(config_clone, tx_inner).await; });
                            last_found_state = true;
                        }
                    } else {
                        if last_found_state {
                            miss_counter += 1;
                            if miss_counter > MISS_TOLERANCE { last_found_state = false; miss_counter = 0; }
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(100)); 
            }
        });

        // Thread Hotkey
        std::thread::spawn(move || {
            let callback = move |event: Event| {
                if LISTENING_PAUSED.load(Ordering::Relaxed) { return; }
                let config = config::Config::load();
                if let EventType::KeyPress(_) = event.event_type {
                    
                    if event.name.as_ref() == Some(&config.hotkey_translate) {
                        let tx = tx_clone.clone();
                        std::thread::spawn(move || {
                            let config = config::Config::load();
                            let rt = tokio::runtime::Runtime::new().unwrap();
                            rt.block_on(async { Self::perform_translation_logic(config, tx).await; });
                        });
                    
                    } else if event.name.as_ref() == Some(&config.hotkey_select) {
                         if !OVERLAY_ACTIVE.load(Ordering::Relaxed) {
                            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
                            if now - LAST_SELECT.load(Ordering::Relaxed) > 1000 {
                                LAST_SELECT.store(now, Ordering::Relaxed);
                                OVERLAY_ACTIVE.store(true, Ordering::Relaxed);
                                overlay::set_selection_mode(0); 
                                std::thread::spawn(|| { overlay::show_selection_overlay(); OVERLAY_ACTIVE.store(false, Ordering::Relaxed); });
                            }
                        }
                    
                    } else if event.name.as_ref() == Some(&config.hotkey_instant) {
                        if !OVERLAY_ACTIVE.load(Ordering::Relaxed) {
                           OVERLAY_ACTIVE.store(true, Ordering::Relaxed);
                           let tx_for_thread = tx_clone.clone();
                           overlay::set_selection_mode(2);
                           std::thread::spawn(move || {
                               overlay::show_selection_overlay();
                               OVERLAY_ACTIVE.store(false, Ordering::Relaxed);
                               
                               let config = config::Config::load();
                               if let Some(instant_region) = config.instant_region.clone() {
                                   let rt = tokio::runtime::Runtime::new().unwrap();
                                   rt.block_on(async { 
                                       Self::perform_instant_translation(config, tx_for_thread, instant_region).await; 
                                   });
                               }
                           });
                       }
                    
                    } else if event.name.as_ref() == Some(&config.hotkey_auto) {
                        if !OVERLAY_ACTIVE.load(Ordering::Relaxed) {
                            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
                            if now - LAST_SELECT.load(Ordering::Relaxed) > 1000 {
                                LAST_SELECT.store(now, Ordering::Relaxed);
                                OVERLAY_ACTIVE.store(true, Ordering::Relaxed);
                                overlay::set_selection_mode(1); 
                                std::thread::spawn(|| { overlay::show_selection_overlay(); OVERLAY_ACTIVE.store(false, Ordering::Relaxed); });
                            }
                        }
                    }
                }
            };
            if let Err(_) = listen(callback) {}
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

        if self.image_texture.is_none() { self.load_texture(ctx, AREA_PLACEHOLDER, false); }
        if self.arrow_texture.is_none() {
             let custom_path = config::Config::get_custom_arrow_path();
             if custom_path.exists() { if let Ok(b) = fs::read(custom_path) { self.load_texture(ctx, &b, true); } } 
             else { self.load_texture(ctx, DEFAULT_ARROW, true); }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading(egui::RichText::new("Instant Screen Narrator").strong().size(24.0));
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
                                        ui.selectable_value(&mut self.selected_api, "groq".to_string(), "Groq (Meta Llama)");
                                    });
                            });
                            
                            if self.selected_api == "groq" {
                                let remaining = GROQ_REMAINING.load(Ordering::Relaxed);
                                if remaining >= 0 {
                                    ui.colored_label(egui::Color32::GREEN, format!("(Còn lại: {} req)", remaining));
                                } else {
                                    ui.colored_label(egui::Color32::GRAY, "(Còn lại: ?)");
                                }
                            }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let help_text = if self.selected_api == "gemini" { "❓ Hướng dẫn (Gemini)" } else { "❓ Hướng dẫn (Groq)" };
                                if ui.add(egui::Button::new(egui::RichText::new(help_text).small())).clicked() {
                                    self.show_popup = true;
                                    self.popup_text = self.selected_api.clone();
                                }
                            });
                        });
                        ui.end_row();

                        ui.label("API Key(s):");
                        ui.vertical(|ui| {
                             if self.selected_api == "gemini" {
                                 ui.add_enabled(!self.started, egui::TextEdit::singleline(&mut self.gemini_api_key).password(!self.show_password).desired_width(400.0));
                             } else {
                                 let mut keys_to_remove = Vec::new();
                                 let show_pass = self.show_password;
                                 let started = self.started;
                                 let keys_count = self.config.groq_api_keys.len(); 

                                 for (i, key) in self.config.groq_api_keys.iter_mut().enumerate() {
                                     ui.horizontal(|ui| {
                                         ui.label(format!("#{}", i + 1));
                                         ui.add_enabled(!started, egui::TextEdit::singleline(key).password(!show_pass).desired_width(350.0));
                                         if !started && keys_count > 1 {
                                             if ui.button("🗑").clicked() { keys_to_remove.push(i); }
                                         }
                                     });
                                 }
                                 for i in keys_to_remove.iter().rev() {
                                     self.config.groq_api_keys.remove(*i);
                                 }
                                 
                                 if !self.started {
                                     if ui.button("➕ Thêm Key dự phòng").clicked() {
                                         self.config.groq_api_keys.push(String::new());
                                     }
                                 }
                             }
                             
                             if ui.button(if self.show_password { "🙈 Ẩn Key" } else { "👁 Hiện Key" }).clicked() {
                                 self.show_password = !self.show_password;
                             }
                        });
                        ui.end_row();
                    });
                });
                
                ui.add_space(5.0);
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

                egui::CollapsingHeader::new(egui::RichText::new("⌨️ Phím tắt").strong()).default_open(true).show(ui, |ui| {
                     egui::Grid::new("hotkey_grid").num_columns(2).spacing([20.0, 10.0]).striped(true).show(ui, |ui| {
                        ui.label("Dịch vùng đã chọn:"); ui.add_enabled(!self.started, egui::TextEdit::singleline(&mut self.hotkey_translate).desired_width(80.0)); ui.end_row();
                        ui.label("Chọn vùng dịch:"); ui.add_enabled(!self.started, egui::TextEdit::singleline(&mut self.hotkey_select).desired_width(80.0)); ui.end_row();
                        ui.label("Chụp & Dịch ngay:"); ui.add_enabled(!self.started, egui::TextEdit::singleline(&mut self.hotkey_instant).desired_width(80.0)); ui.end_row();
                        ui.label("Chọn vùng Mũi tên:"); ui.add_enabled(!self.started, egui::TextEdit::singleline(&mut self.hotkey_auto).desired_width(80.0)); ui.end_row();
                    });
                });
                ui.add_space(5.0);

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

                ui.vertical_centered(|ui| {
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Center), |ui| {
                            // ĐỔI TÊN Ở ĐÂY
                            let mut wwm_text = "🎯 Tự động chọn vùng dịch WWM";
                            if let Some(time) = self.wwm_success_timer {
                                if time.elapsed().as_secs_f32() < 1.0 { wwm_text = "✅ Đã chọn"; ctx.request_repaint(); } 
                                else { self.wwm_success_timer = None; }
                            }
                            if ui.add_enabled(self.started, egui::Button::new(wwm_text)).clicked() {
                                let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) } as f32;
                                let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) } as f32;
                                let r_x = 308.0 / 1920.0; let r_y = 919.0 / 1080.0;
                                let r_w = 1313.0 / 1920.0; let r_h = 135.0 / 1080.0;
                                let region = config::Region { x: (screen_w * r_x) as i32, y: (screen_h * r_y) as i32, width: (screen_w * r_w) as u32, height: (screen_h * r_h) as u32 };
                                self.config.fixed_regions.clear(); self.config.fixed_regions.push(region.clone());
                                self.config.save().unwrap();
                                self.wwm_success_timer = Some(std::time::Instant::now());
                                let rect = RECT{left: region.x, top: region.y, right: region.x + region.width as i32, bottom: region.y + region.height as i32};
                                overlay::show_highlight(rect);
                            }
                            ui.label(egui::RichText::new("(Dành cho màn 16:9)").italics().color(egui::Color32::GRAY));
                            if ui.button("🖼️ Ảnh").clicked() { self.show_image_window = true; }
                        });
                    });
                    ui.add_space(5.0);

                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Center), |ui| {
                            let mut arrow_text = "🏹 Tự động chọn vùng Mũi tên WWM";
                            if let Some(time) = self.arrow_wwm_success_timer {
                                if time.elapsed().as_secs_f32() < 1.0 { arrow_text = "✅ Đã chọn"; ctx.request_repaint(); }
                                else { self.arrow_wwm_success_timer = None; }
                            }
                            if ui.add_enabled(self.started, egui::Button::new(arrow_text)).clicked() {
                                let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) } as f32;
                                let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) } as f32;
                                let r_x = 930.0 / 1920.0; let r_y = 1042.0 / 1080.0;
                                let r_w = 49.0 / 1920.0; let r_h = 36.0 / 1080.0;
                                let region = config::Region { x: (screen_w * r_x) as i32, y: (screen_h * r_y) as i32, width: (screen_w * r_w) as u32, height: (screen_h * r_h) as u32 };
                                self.config.arrow_region = Some(region.clone());
                                self.config.save().unwrap();
                                self.arrow_wwm_success_timer = Some(std::time::Instant::now());
                                let rect = RECT{left: region.x, top: region.y, right: region.x + region.width as i32, bottom: region.y + region.height as i32};
                                overlay::show_highlight(rect);
                            }
                            ui.label(egui::RichText::new("(Dành cho màn 16:9)").italics().color(egui::Color32::GRAY));
                            if ui.button("🖼️ Ảnh").clicked() { self.show_arrow_window = true; }
                        });
                    });

                    ui.add_space(10.0);

                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Center), |ui| {
                            if self.started {
                                if ui.toggle_value(&mut self.auto_translate_active, "🔄 Tự động dịch").changed() {
                                    AUTO_TRANSLATE_ENABLED.store(self.auto_translate_active, Ordering::Relaxed);
                                }
                                if self.auto_translate_active {
                                    ui.label(egui::RichText::new("Đang chạy...").color(egui::Color32::GREEN));
                                }
                                
                                ui.separator();
                                
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
                                        self.load_texture(ctx, DEFAULT_ARROW, true);
                                    }
                                }
                            } else {
                                ui.add_enabled(false, egui::Button::new("🔄 Tự động dịch"));
                            }
                        });
                    });
                    
                    if self.started {
                        ui.label(egui::RichText::new("Lưu ý: Nếu đổi ảnh, hãy Tắt/Bật lại Dịch Tự Động.").small().italics());
                    }

                    ui.add_space(10.0);

                    if !self.started {
                        let start_btn = egui::Button::new(egui::RichText::new("🚀 BẮT ĐẦU SỬ DỤNG").size(20.0).strong().color(egui::Color32::WHITE))
                            .min_size(egui::vec2(200.0, 50.0)).fill(egui::Color32::from_rgb(0, 120, 215)); 
                        if ui.add(start_btn).clicked() {
                            self.config.gemini_api_key = self.gemini_api_key.clone();
                            self.config.current_prompt = self.current_prompt.clone();
                            self.config.hotkey_translate = self.hotkey_translate.clone();
                            self.config.hotkey_select = self.hotkey_select.clone();
                            self.config.hotkey_instant = self.hotkey_instant.clone();
                            self.config.hotkey_auto = self.hotkey_auto.clone();
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
                        if ui.add(stop_btn).clicked() { 
                            self.started = false; 
                            self.auto_translate_active = false;
                            AUTO_TRANSLATE_ENABLED.store(false, Ordering::Relaxed);
                            LISTENING_PAUSED.store(true, Ordering::Relaxed); 
                        }
                    }
                    ui.add_space(10.0);
                    ui.colored_label(egui::Color32::GRAY, "ℹ Chạy Admin nếu muốn dịch game Fullscreen");
                });
            });
        });

        if self.show_popup {
            let mut open = true;
            let title = if self.popup_text == "gemini" { "Hướng dẫn Gemini" } else { "Hướng dẫn Groq" };
            egui::Window::new(title).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
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

        if self.show_image_window {
            let mut open = true;
            egui::Window::new("Khu vực dịch (16:9)").open(&mut open).collapsible(false).resizable(true).default_size(egui::vec2(600.0, 400.0)).show(ctx, |ui| {
                if let Some(texture) = &self.image_texture {
                    let s = texture.size_vec2();
                    let scale = (ui.available_width() / s.x).min(ui.available_height() / s.y);
                    ui.centered_and_justified(|ui| { ui.image((texture.id(), s * scale)); });
                } else { ui.label("Không tìm thấy area.png"); }
            });
            if !open { self.show_image_window = false; }
        }

        if self.show_arrow_window {
            let mut open = true;
            egui::Window::new("Mũi tên hiện tại").open(&mut open).collapsible(false).resizable(true).default_size(egui::vec2(300.0, 300.0)).show(ctx, |ui| {
                 ui.label("Đây là hình ảnh mũi tên đang được dùng để nhận diện:");
                 ui.separator();
                if let Some(texture) = &self.arrow_texture {
                    let s = texture.size_vec2() * 2.0;
                    ui.centered_and_justified(|ui| { ui.image((texture.id(), s)); });
                } else { ui.label("Không tìm thấy arrow.png"); }
            });
            if !open { self.show_arrow_window = false; }
        }
    }
}

fn main() -> Result<(), eframe::Error> {
    let mut options = eframe::NativeOptions::default();
    options.viewport.transparent = Some(false);
    options.viewport.inner_size = Some(egui::vec2(900.0, 950.0));
    options.viewport.taskbar = Some(true);

    let icon_bytes = include_bytes!("icon2.ico");
    if let Ok(icon_image) = image::load_from_memory(icon_bytes) {
        let icon_rgba = icon_image.to_rgba8();
        let icon_width = icon_image.width() as u32;
        let icon_height = icon_image.height() as u32;
        let icon_data = egui::IconData {
            rgba: icon_rgba.into_raw(),
            width: icon_width,
            height: icon_height,
        };
        options.viewport.icon = Some(icon_data.into());
    }

    eframe::run_native(
        "Instant Screen Narrator",
        options,
        Box::new(|_cc| Box::new(MainApp::default())),
    )
}