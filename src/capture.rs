use image::{DynamicImage, ImageFormat};
use screenshots::Screen;

pub fn capture_image(region: &crate::config::Region) -> Result<Vec<u8>, anyhow::Error> {
    let screens = Screen::all()?;
    if screens.is_empty() { return Err(anyhow::anyhow!("No screens found")); }
    let screen = &screens[0];
    let image = screen.capture_area(region.x as i32, region.y as i32, region.width, region.height)?;
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
    let screen = &screens[0];

    // 1. Capture screen
    let captured_image = match screen.capture_area(region.x as i32, region.y as i32, region.width, region.height) {
        Ok(img) => DynamicImage::ImageRgba8(img),
        Err(_) => return false,
    };

    // 2. Load template
    let template_image = match image::load_from_memory(template_bytes) {
        Ok(img) => img.to_rgba8(),
        Err(_) => return false,
    };

    let haystack = captured_image.to_rgba8();
    let (w_h, h_h) = haystack.dimensions();
    let (w_n, h_n) = template_image.dimensions();

    if w_h < w_n || h_h < h_n { return false; }

    // === SENSITIVITY CONFIG ===
    let color_tolerance = 60; 
    let match_threshold = 0.85; 
    // =========================

    let limit_x = w_h - w_n;
    let limit_y = h_h - h_n;

    for y in 0..=limit_y {
        for x in 0..=limit_x {
            if quick_check(&haystack, &template_image, x, y, w_n, h_n, color_tolerance) {
                if fuzzy_match(&haystack, &template_image, x, y, color_tolerance, match_threshold) {
                    return true;
                }
            }
        }
    }
    
    false
}

fn quick_check(haystack: &image::RgbaImage, needle: &image::RgbaImage, sx: u32, sy: u32, w: u32, h: u32, tol: i16) -> bool {
    let cx = w / 2;
    let cy = h / 2;
    let p_n = needle.get_pixel(cx, cy);
    if p_n[3] < 10 { return true; }
    let p_h = haystack.get_pixel(sx + cx, sy + cy);
    pixel_diff(p_h, p_n) <= tol
}

fn fuzzy_match(haystack: &image::RgbaImage, needle: &image::RgbaImage, sx: u32, sy: u32, tol: i16, threshold: f32) -> bool {
    let (w, h) = needle.dimensions();
    let mut total_checked = 0;
    let mut matched_count = 0;

    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            let p_n = needle.get_pixel(x, y);
            if p_n[3] < 20 { continue; }
            total_checked += 1;
            let p_h = haystack.get_pixel(sx + x, sy + y);
            if pixel_diff(p_h, p_n) <= tol { matched_count += 1; }
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