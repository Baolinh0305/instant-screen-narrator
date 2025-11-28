use winapi::um::winuser::*;

pub fn get_vk_from_name(name: &str) -> i32 {
    let name = name.to_uppercase();
    match name.as_str() {
        "F1" => VK_F1, "F2" => VK_F2, "F3" => VK_F3, "F4" => VK_F4,
        "F5" => VK_F5, "F6" => VK_F6, "F7" => VK_F7, "F8" => VK_F8,
        "F9" => VK_F9, "F10" => VK_F10, "F11" => VK_F11, "F12" => VK_F12,
        "SPACE" => VK_SPACE, "ENTER" => VK_RETURN, "TAB" => VK_TAB, "ESC" => VK_ESCAPE,
        "SHIFT" => VK_SHIFT, "CTRL" => VK_CONTROL, "ALT" => VK_MENU,
        "INSERT" => VK_INSERT, "DELETE" => VK_DELETE, "HOME" => VK_HOME, "END" => VK_END,
        "PAGEUP" => VK_PRIOR, "PAGEDOWN" => VK_NEXT,
        "UP" => VK_UP, "DOWN" => VK_DOWN, "LEFT" => VK_LEFT, "RIGHT" => VK_RIGHT,
        "[" => 0xDB, "]" => 0xDD, "\\" => 0xDC, ";" => 0xBA, "'" => 0xDE,
        "," => 0xBC, "." => 0xBE, "/" => 0xBF, "`" => 0xC0, "-" => 0xBD, "=" => 0xBB,
        _ => {
            if name.len() == 1 {
                let c = name.chars().next().unwrap();
                if c >= '0' && c <= '9' { return c as i32; }
                if c >= 'A' && c <= 'Z' { return c as i32; }
            }
            0
        }
    }
}

pub fn get_name_from_vk(vk: i32) -> String {
    match vk {
        VK_F1 => "F1".to_string(), VK_F2 => "F2".to_string(), VK_F3 => "F3".to_string(), VK_F4 => "F4".to_string(),
        VK_F5 => "F5".to_string(), VK_F6 => "F6".to_string(), VK_F7 => "F7".to_string(), VK_F8 => "F8".to_string(),
        VK_F9 => "F9".to_string(), VK_F10 => "F10".to_string(), VK_F11 => "F11".to_string(), VK_F12 => "F12".to_string(),
        VK_SPACE => "SPACE".to_string(), VK_RETURN => "ENTER".to_string(), VK_TAB => "TAB".to_string(), VK_ESCAPE => "ESC".to_string(),
        VK_SHIFT | VK_LSHIFT | VK_RSHIFT => "SHIFT".to_string(),
        VK_CONTROL | VK_LCONTROL | VK_RCONTROL => "CTRL".to_string(),
        VK_MENU | VK_LMENU | VK_RMENU => "ALT".to_string(),
        VK_INSERT => "INSERT".to_string(), VK_DELETE => "DELETE".to_string(), VK_HOME => "HOME".to_string(), VK_END => "END".to_string(),
        VK_PRIOR => "PAGEUP".to_string(), VK_NEXT => "PAGEDOWN".to_string(),
        VK_UP => "UP".to_string(), VK_DOWN => "DOWN".to_string(), VK_LEFT => "LEFT".to_string(), VK_RIGHT => "RIGHT".to_string(),
        0xDB => "[".to_string(), 0xDD => "]".to_string(), 0xDC => "\\".to_string(), 0xBA => ";".to_string(), 0xDE => "'".to_string(),
        0xBC => ",".to_string(), 0xBE => ".".to_string(), 0xBF => "/".to_string(), 0xC0 => "`".to_string(), 0xBD => "-".to_string(), 0xBB => "=".to_string(),
        _ => {
            if vk >= '0' as i32 && vk <= '9' as i32 { return ((vk as u8) as char).to_string(); }
            if vk >= 'A' as i32 && vk <= 'Z' as i32 { return ((vk as u8) as char).to_string(); }
            format!("KEY_{}", vk)
        }
    }
}