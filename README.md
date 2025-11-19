# Instant Screen Narrator

Ứng dụng dịch màn hình tức thời với khả năng đọc văn bản bằng giọng nói.

## Tính năng

- **Dịch màn hình tức thời**: Chụp ảnh vùng màn hình đã chọn và dịch sang tiếng Việt.
- **Đọc văn bản**: Chuyển văn bản đã dịch thành giọng nói.
- **Hỗ trợ nhiều API**: Gemini (Google) và Groq (nhanh hơn, giới hạn 1000 lần/ngày).
- **Giao diện đơn giản**: Dễ sử dụng với các tùy chọn cơ bản.
- **Chạy ẩn**: Ứng dụng chạy ở chế độ cửa sổ trong suốt.

## Yêu cầu hệ thống

- Windows 10/11
- Kết nối internet (để sử dụng API dịch)

## Cài đặt

1. Tải file `screen_translator.exe` từ releases.
2. Chạy file exe (không cần cài đặt gì thêm).

## Sử dụng

1. Mở ứng dụng.
2. Chọn API dịch (Groq khuyến nghị vì nhanh).
3. Nhập API key tương ứng.
4. Chọn prompt dịch (bình thường hoặc kiểu kiếm hiệp).
5. Đặt phím tắt (mặc định [ để dịch, ] để chọn vùng).
6. Nhấn "Bắt đầu".
7. Sử dụng phím tắt để chọn vùng màn hình và dịch.

## Lưu ý

- Cần chạy với quyền quản trị để dịch game hoặc app fullscreen.
- API key được lưu cục bộ trên máy.

## Phát triển

Dự án được viết bằng Rust sử dụng eframe cho GUI.

Để build:

```bash
cargo build --release
```

## Giấy phép

MIT License