use image::{DynamicImage, ImageFormat, imageops::FilterType, RgbaImage};
use screenshots::Screen;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};

// Biến toàn cục để lưu lại "kích thước mũi tên đúng nhất"
// Giúp lần sau không phải dò lại từ đầu -> Tiết kiệm CPU
static CACHED_TEMPLATE: Mutex<Option<RgbaImage>> = Mutex::new(None);

// Bộ đếm để hạn chế tần suất quét kỹ (tránh nổ máy khi không có mũi tên)
static SCAN_COUNTER: AtomicU8 = AtomicU8::new(0);

/// Tìm màn hình chứa điểm (x, y)
fn find_screen_containing(x: i32, y: i32, screens: &[Screen]) -> Option<&Screen> {
    screens.iter().find(|s| {
        let info = s.display_info;
        x >= info.x 
        && x < info.x + info.width as i32
        && y >= info.y 
        && y < info.y + info.height as i32
    })
}

pub fn capture_image(region: &crate::config::Region) -> Result<Vec<u8>, anyhow::Error> {
    let screens = Screen::all()?;
    if screens.is_empty() { return Err(anyhow::anyhow!("No screens found")); }
    
    let screen = find_screen_containing(region.x, region.y, &screens)
        .unwrap_or(&screens[0]);

    let relative_x = region.x - screen.display_info.x;
    let relative_y = region.y - screen.display_info.y;

    let image = screen.capture_area(relative_x, relative_y, region.width, region.height)?;
    let img = DynamicImage::ImageRgba8(image);
    let mut buffer = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buffer), ImageFormat::Png)?;
    Ok(buffer)
}

pub fn is_template_present(region: &crate::config::Region, template_bytes: &[u8]) -> bool {
    let screens = match Screen::all() {
        Ok(s) => s,
        Err(_) => return false,
    };
    if screens.is_empty() { return false; }
    
    // 1. Chụp màn hình (đúng màn hình chứa vùng chọn)
    let screen = find_screen_containing(region.x, region.y, &screens)
        .unwrap_or(&screens[0]);

    let relative_x = region.x - screen.display_info.x;
    let relative_y = region.y - screen.display_info.y;

    let captured_image = match screen.capture_area(relative_x, relative_y, region.width, region.height) {
        Ok(img) => DynamicImage::ImageRgba8(img),
        Err(_) => return false,
    };

    let haystack = captured_image.to_rgba8();

    // 2. Load template gốc từ bytes
    let template_original = match image::load_from_memory(template_bytes) {
        Ok(img) => img.to_rgba8(),
        Err(_) => return false,
    };

    // --- CHIẾN LƯỢC TIẾT KIỆM CPU ---

    // BƯỚC 1: Kiểm tra Cache (Cái đã thành công lần trước)
    // Nếu lần trước kích thước 1.2x đúng, giờ thử nó đầu tiên.
    {
        let cached = CACHED_TEMPLATE.lock().unwrap();
        if let Some(ref cached_img) = *cached {
            if check_match_at_scale(&haystack, cached_img) {
                // Reset bộ đếm vì đã tìm thấy
                SCAN_COUNTER.store(0, Ordering::Relaxed);
                return true;
            }
        }
    }

    // BƯỚC 2: Kiểm tra kích thước gốc (1.0x)
    // Nếu Cache sai (do game đổi size), thử lại bản gốc.
    if check_match_at_scale(&haystack, &template_original) {
        // Nếu bản gốc đúng, lưu vào Cache
        let mut cached = CACHED_TEMPLATE.lock().unwrap();
        *cached = Some(template_original);
        SCAN_COUNTER.store(0, Ordering::Relaxed);
        return true;
    }

    // BƯỚC 3: Quét kỹ (Multi-scale) - CHỈ CHẠY KHI CẦN THIẾT
    // Chỉ chạy mỗi 10 frame (ví dụ: 0.2s một lần) để không nổ máy
    let counter = SCAN_COUNTER.fetch_add(1, Ordering::Relaxed);
    if counter % 10 != 0 {
        return false; // Skip, đợi lượt sau
    }

    // Các tỷ lệ cần thử: 90%, 110%, 80%, 120%, 130%
    let scales = [0.9, 1.1, 0.8, 1.2, 1.3]; 
    
    for scale in scales {
        let (orig_w, orig_h) = template_original.dimensions();
        let new_w = (orig_w as f32 * scale) as u32;
        let new_h = (orig_h as f32 * scale) as u32;

        if new_w < 5 || new_h < 5 { continue; }

        let template_resized = image::imageops::resize(
            &template_original, 
            new_w, 
            new_h, 
            FilterType::Lanczos3
        );

        if check_match_at_scale(&haystack, &template_resized) {
            // TÌM THẤY! Lưu ngay kích thước này vào Cache
            let mut cached = CACHED_TEMPLATE.lock().unwrap();
            *cached = Some(template_resized);
            SCAN_COUNTER.store(0, Ordering::Relaxed);
            return true;
        }
    }
    
    // Vẫn không thấy
    false
}

// Hàm so khớp logic (Đã tối ưu)
fn check_match_at_scale(haystack: &RgbaImage, needle: &RgbaImage) -> bool {
    let (w_h, h_h) = haystack.dimensions();
    let (w_n, h_n) = needle.dimensions();

    if w_h < w_n || h_h < h_n { return false; }

    // Tăng dung sai lên chút để dễ bắt hơn
    let color_tolerance = 70; 
    let match_threshold = 0.80; // Giảm threshold xuống 80% cho dễ

    let limit_x = w_h - w_n;
    let limit_y = h_h - h_n;

    // Tối ưu: Chỉ quét vùng trung tâm nếu có thể, nhưng để an toàn cứ quét hết
    // Tăng bước nhảy (step) khi quét sơ bộ để nhanh hơn
    for y in (0..=limit_y).step_by(2) {
        for x in (0..=limit_x).step_by(2) {
            if quick_check(haystack, needle, x, y, w_n, h_n, color_tolerance) {
                if fuzzy_match(haystack, needle, x, y, color_tolerance, match_threshold) {
                    return true;
                }
            }
        }
    }
    false
}

fn quick_check(haystack: &RgbaImage, needle: &RgbaImage, sx: u32, sy: u32, w: u32, h: u32, tol: i16) -> bool {
    let cx = w / 2;
    let cy = h / 2;
    let p_n = needle.get_pixel(cx, cy);
    if p_n[3] < 10 { return true; }
    let p_h = haystack.get_pixel(sx + cx, sy + cy);
    pixel_diff(p_h, p_n) <= tol
}

fn fuzzy_match(haystack: &RgbaImage, needle: &RgbaImage, sx: u32, sy: u32, tol: i16, threshold: f32) -> bool {
    let (w, h) = needle.dimensions();
    let mut total_checked = 0;
    let mut matched_count = 0;

    // Bước nhảy 2: Giảm 4 lần lượng tính toán
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            let p_n = needle.get_pixel(x, y);
            if p_n[3] < 20 { continue; } // Bỏ qua pixel trong suốt
            
            total_checked += 1;
            let p_h = haystack.get_pixel(sx + x, sy + y);
            
            if pixel_diff(p_h, p_n) <= tol { 
                matched_count += 1; 
            }
        }
    }

    if total_checked == 0 { return true; }
    (matched_count as f32 / total_checked as f32) >= threshold
}

fn pixel_diff(p1: &image::Rgba<u8>, p2: &image::Rgba<u8>) -> i16 {
    let r = (p1[0] as i16 - p2[0] as i16).abs();
    let g = (p1[1] as i16 - p2[1] as i16).abs();
    let b = (p1[2] as i16 - p2[2] as i16).abs();
    r.max(g).max(b)
}