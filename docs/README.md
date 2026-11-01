# Tài liệu kỹ thuật ctdozlite

Thư mục này chứa tài liệu chi tiết về cách sử dụng, cấu trúc mã nguồn và giao thức mạng của `ctdozlite` (`zalo-rust`).

## Danh mục tài liệu

| Tài liệu | Nội dung |
|---|---|
| [1. Cài đặt](CAI_DAT.md) | Tải và cài đặt trên Linux, Windows, macOS; kiểm tra SHA-256 và gỡ cài đặt. |
| [2. Hướng dẫn sử dụng](HUONG_DAN_SU_DUNG.md) | Các tham số CLI, thao tác trên GUI (`egui`), phím tắt TUI (`ratatui`) và công cụ kiểm thử. |
| [3. Kiến trúc hệ thống](KIEN_TRUC_HE_THONG.md) | Mô hình phân tách UI thread / Tokio worker, đồng bộ WebSocket + HTTP polling và lưu trữ SQLite. |
| [4. Đặc tả giao thức Zalo](GIAO_THUC_ZALO.md) | Cấu trúc tham số `zpw_ver=694`, `ParamCipher v2`, `PasswordCipher`, `signkey` và cơ chế mã hóa AES. |
| [5. Xử lý sự cố](KHAC_PHUC_SU_CO.md) | Bảng mã lỗi máy chủ Zalo (`18012`, `18060`, `18022`, `600`, `102`, `114`, `429`) và cách khắc phục. |

## Biên dịch và chạy nhanh

```bash
git clone https://github.com/ctdoteam1110/ctdozlite.git
cd ctdozlite
cargo build --release

# Chạy giao diện đồ họa (mặc định)
./target/release/zalo-rust --gui

# Chạy giao diện terminal
./target/release/zalo-rust --tui --qr

# Bật thêm bảng kiểm tra mạng (Dev Inspector)
./target/release/zalo-rust --gui --dev
```
