extern crate winres;

fn main() {
    if cfg!(target_os = "windows") {
        let mut res = winres::WindowsResource::new();
        
        // 1. Giữ nguyên icon của bạn
        res.set_icon("src/icon2.ico");
        
        // 2. THÊM MỚI: Nhúng Manifest để bắt buộc chạy Admin
        res.set_manifest(r#"
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
<trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
        <requestedPrivileges>
            <requestedExecutionLevel level="requireAdministrator" uiAccess="false" />
        </requestedPrivileges>
    </security>
</trustInfo>
</assembly>
"#);

        res.compile().unwrap();
    }
}