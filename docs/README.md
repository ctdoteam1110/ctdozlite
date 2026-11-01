# Tài liệu QTzalo

Tài liệu cho người dùng và người phát triển QTzalo (`zalo-rust`).

## Cho người dùng

| Tài liệu | Nội dung |
|---|---|
| [Cài đặt](CAI_DAT.md) | Tải và cài trên Windows / Linux / macOS, kiểm tra SHA256, xử lý SmartScreen, gỡ cài đặt. |
| [Hướng dẫn sử dụng](HUONG_DAN_SU_DUNG.md) | Tham số dòng lệnh, thao tác GUI, phím tắt TUI. |
| [Xử lý sự cố](KHAC_PHUC_SU_CO.md) | Mã lỗi máy chủ Zalo, font, phiên đăng nhập, giờ hiển thị, lỗi build. |

## Cho người phát triển

| Tài liệu | Nội dung |
|---|---|
| [Kiến trúc hệ thống](KIEN_TRUC_HE_THONG.md) | Phân tách UI thread / Tokio worker, WebSocket + HTTP polling, lưu trữ SQLite. |
| [Đặc tả giao thức Zalo](GIAO_THUC_ZALO.md) | `zpw_ver=694`, `ParamCipher v2`, `PasswordCipher`, `signkey`, mã hoá AES. |
| [Rà soát bảo mật](SECURITY_AUDIT.md) | Các vấn đề bảo mật và lỗi chức năng đã phát hiện, trạng thái khắc phục. |
| [Kế hoạch production](KE_HOACH_PRODUCTION.md) | Lộ trình kế thừa từ dự án gốc. |
| [../re-notes/](../re-notes/) | Ghi chú reverse-engineering giao thức (đăng nhập, dữ liệu chat, cuộc gọi). |

## Build và chạy nhanh

Yêu cầu Rust ≥ 1.88, CMake, `pkg-config`, trình biên dịch C. Chi tiết theo từng hệ điều hành xem [README](../README.md#tự-build-từ-mã-nguồn).

```bash
git clone https://github.com/quangthoai1985/QTzalo.git
cd QTzalo
CMAKE_POLICY_VERSION_MINIMUM=3.5 cargo build --release --locked
./target/release/zalo-rust
```

Các lệnh hữu ích khi phát triển:

```bash
cargo test --locked
```

```bash
scripts/check-windows.sh
```

```bash
./target/release/zalo-rust --gui --dev
```
