# Nhật ký thay đổi

Mọi thay đổi đáng chú ý của QTzalo được ghi tại đây. Định dạng theo [Keep a Changelog](https://keepachangelog.com/vi/1.1.0/), phiên bản theo [Semantic Versioning](https://semver.org/lang/vi/).

QTzalo là bản fork của [ctdozlite](https://github.com/ctdoteam1110/ctdozlite), tách ra từ commit `73849bd` (tag `v0.0.2` của upstream).

## [0.1.0] - 2026-11-01

Phiên bản đầu tiên của QTzalo.

### Bảo mật

- **Lưu dữ liệu an toàn**
  - `session.json`, lịch sử `zalo_history.db`, `threads_cache.json`, `app_settings.json`, cache ảnh, log và dump nay nằm trong một thư mục riêng của người dùng:
    - Windows: `%APPDATA%\QTzalo`
    - Linux: `~/.local/share/qtzalo`
    - macOS: `~/Library/Application Support/QTzalo`
  - Trước đây các file này nằm ở thư mục đang đứng khi chạy app.
  - File chứa thông tin đăng nhập được ghi nguyên tử với quyền `0600`, thư mục có quyền `0700`.
  - File của bản cũ được tự động chuyển sang khi khởi động.
  - Đặt biến `QTZALO_DATA_DIR` để đổi vị trí lưu.
- **Tắt log chẩn đoán mặc định:** `call_trace.log` (chứa payload báo hiệu cuộc gọi đã giải mã) và `voip.log` chỉ được ghi khi chạy `--dev` hoặc đặt `ZALO_CALL_TRACE=1` / `ZALO_VOIP_LOG`.
- **Link preview không còn làm lộ IP**
  - Tin nhắn đến chỉ dùng kết quả `parselink` phía máy chủ Zalo.
  - App chỉ truy cập trực tiếp với link do chính bạn gửi: chỉ `https`, chặn IP nội bộ/loopback, ghim DNS, kiểm tra lại từng lần chuyển hướng, giới hạn 256 KiB.
  - Ảnh: chỉ tải qua `https` (`http` chỉ cho CDN Zalo), giới hạn 15 MiB, không đọc đường dẫn mạng/UNC.
- **Sinh số ngẫu nhiên an toàn:** IMEI/device-id, session cuộc gọi, salt mật khẩu và khoá tham số dùng nguồn ngẫu nhiên của hệ điều hành (`getrandom`) thay cho bộ sinh theo thời gian.
- **WebSocket**
  - Cookie phiên chỉ gửi tới `wss://` thuộc domain Zalo; địa chỉ `ws://` không mã hoá bị loại.
  - `ZCALL_WS` chỉ có hiệu lực ở chế độ `--dev`.
  - Dữ liệu giải nén giới hạn 32 MiB.
  - Cookie HTTP thêm cờ `Secure`.
- **Mật khẩu**
  - Bỏ tham số `--pass <mật khẩu>` (lộ trong danh sách tiến trình), thêm `--pass-stdin`.
  - Ô nhập mật khẩu trong terminal không còn hiện ký tự.
- **Dependency**
  - Nâng `tokio` lên 1.54.1 (vá RUSTSEC-2026-0384) và `clipboard-win` lên 5.4.3 (bản cũ đã bị yank).
  - `cargo audit` báo 0 lỗ hổng.
  - Còn 5 cảnh báo: `lru` unsound qua ratatui 0.29; `audiopus_sys`, `paste`, `ttf-parser` hết bảo trì.
- Xoá đường dẫn máy phát triển còn sót (`F:\CODE\...`) và các đường dẫn ZaloCall.exe tương đối.

### Sửa lỗi

- **Mất phiên đăng nhập khi rớt mạng (#21, #3)**
  - Chỉ mã lỗi xác thực rõ ràng (`600`, `-102`) mới bị coi là hết hạn.
  - Lỗi mạng, lỗi HTTP hay phản hồi không đọc được: giữ nguyên phiên và báo "Không kết nối được máy chủ Zalo".
  - Phiên hết hạn chỉ bị thay thế khi đăng nhập mới thành công.
- **Đăng xuất không còn xoá lịch sử tin nhắn cục bộ** (`zalo_history.db`).
- **Danh sách bạn bè lỗi 404 (#22, #3)**
  - Domain đã kiểm chứng luôn được ưu tiên; `zpw_service_map_v3` từ máy chủ chỉ bổ sung domain còn thiếu.
  - Khi nạp phiên, toàn bộ domain được khôi phục (trước chỉ 3/13).
  - `get_friends` thử host `friend` trước và hiện lỗi thật thay vì danh sách rỗng.
- **Sai giờ hiển thị (#4):** giờ tin nhắn, danh sách hội thoại và log dùng múi giờ của máy (có xử lý giờ mùa hè) thay vì UTC.
- **Gỡ khoá gọi thử nghiệm**
  - Trước đây chỉ gọi được liên hệ có tên chứa "hoang", "do thanh" hoặc "dtc"; nay gọi được mọi liên hệ 1-1.
  - Gọi nhóm báo rõ "Chưa hỗ trợ gọi nhóm".
- Đăng nhập bằng mật khẩu ở chế độ TUI giờ lưu phiên đăng nhập.
- Sửa lỗi tràn số khi cắt ảnh đại diện.

### Giao diện

<!-- Điền khi tích hợp: bố cục 3 cột mới, thu gọn cột thành biểu tượng, theme sáng. -->

### Build & phát hành

- Workflow CI mới (`.github/workflows/ci-release.yml`):
  - Lấy phiên bản từ `Cargo.toml` cho mọi tên file và metadata; tag phải khớp phiên bản.
  - Phát hành bằng nút *Run workflow* (tick `release`) hoặc push tag `v<phiên bản>`.
  - Quyền token mặc định chỉ đọc; chỉ job phát hành có quyền ghi.
  - Mọi action bên thứ ba được ghim theo commit SHA.
  - Bỏ bản AppImage, vì `appimagetool` không có bản phát hành cố định để kiểm tra checksum.
  - Release chỉ được tạo khi test và bản Windows build thành công, kèm `SHA256SUMS.txt`.
- Bộ cài Windows (Inno Setup) đổi thương hiệu thành **QTzalo**:
  - `AppId` mới (không đụng bản ctdozlite đã cài) và cài file chạy thành `QTzalo.exe`.
  - Không cần quyền Administrator; yêu cầu Windows 10 trở lên.
  - Khi gỡ chỉ xoá dữ liệu người dùng nếu được đồng ý.
- Bản Windows link tĩnh C runtime (`+crt-static`) nên chạy được trên máy chưa cài Visual C++ Redistributable.
- `build.rs` nhúng icon theo **máy đích** (`CARGO_CFG_TARGET_OS`) thay vì máy build; hỗ trợ MinGW qua `windres`.
- Thêm `scripts/check-windows.sh` để kiểm tra mã dành riêng cho Windows ngay trên Linux.
- Gói Linux đổi tên thành `qtzalo` (`.deb`, `.tar.gz`). `install.sh` hỗ trợ `--user`, `uninstall.sh` hỗ trợ `--purge`.
- Xoá thư mục `web-bundle/` (JavaScript đã làm rối của Zalo, không dùng khi build).

### Tài liệu

- Viết lại `README.md` cho QTzalo: yêu cầu **Rust ≥ 1.88** (đã kiểm chứng 1.87 không build được), vị trí lưu dữ liệu, quy trình phát hành, ghi công dự án gốc.
- Thêm `docs/CAI_DAT.md` (hướng dẫn cài đặt, xử lý SmartScreen), `docs/SECURITY_AUDIT.md` (kết quả rà soát), `SECURITY.md`.
- Bổ sung `docs/KHAC_PHUC_SU_CO.md`: font, phiên đăng nhập, giờ hiển thị.

[0.1.0]: https://github.com/quangthoai1985/QTzalo/releases/tag/v0.1.0
