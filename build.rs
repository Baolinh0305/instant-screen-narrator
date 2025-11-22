extern crate winres;

fn main() {
    if cfg!(target_os = "windows") {
        let mut res = winres::WindowsResource::new();
        res.set_icon("src/icon2.ico");
        res.compile().unwrap();
    }
}