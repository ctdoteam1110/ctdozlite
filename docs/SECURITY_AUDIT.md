# Rà soát bảo mật và chất lượng — QTzalo

- **Phạm vi:** toàn bộ mã nguồn tại commit `73849bd` (trùng tag `v0.0.2` của upstream ctdozlite), dependency trong `Cargo.lock`, workflow CI và các bản phát hành của upstream.
- **Ngày rà soát:** 2026-11-01.
- **Phương pháp:**
  - Đọc kỹ các module bảo mật: `session`, `login`, `qr`, `crypto`, `param_cipher`, `socket`, `store`, `logger`.
  - Quét các module lớn (`chat`, `call`, `media`, `gui/*`) để tìm chỗ gọi lệnh hệ thống, ghi file, mở URL và đọc gói tin mạng.
  - Chạy `cargo build`, `cargo test` (95 test qua) và `cargo audit`.
  - Build thử với Rust 1.87 và 1.88 để xác định phiên bản tối thiểu.

Cột **Trạng thái** được cập nhật khi tích hợp bản sửa: `Đang xử lý` → `Đã sửa (<commit>)` / `Chấp nhận rủi ro` / `Không sửa được phía client`.

## 1. Bảo mật

| # | Mức | Vấn đề | Vị trí (tại `73849bd`) | Hướng khắc phục | Trạng thái |
|---|---|---|---|---|---|
| S1 | Cao | Token phiên (`zpw_sek`, `zpw_enk`, khoá phiên) lưu dạng văn bản thô trong `session.json`, quyền file mặc định (0644 trên Linux/macOS), ghi vào thư mục làm việc hiện tại. Lịch sử chat `zalo_history.db` cũng không bảo vệ. | `src/session.rs:356-370`, `src/store.rs`, `src/main.rs` | Thư mục dữ liệu theo người dùng (`src/paths.rs`), ghi nguyên tử với quyền `0600`, thư mục `0700`, tự chuyển dữ liệu cũ. | Đã sửa (`6537b5c`) |
| S2 | Cao | Bản phát hành luôn ghi log chẩn đoán ra đĩa: `call_trace.log` (payload báo hiệu cuộc gọi đã giải mã) và `voip.log` bật mặc định, không phụ thuộc `--dev`. | `src/call.rs:494-506`, `src/voip_log.rs:36-42` | Chỉ ghi khi bật `--dev` hoặc biến môi trường chủ động, ghi vào `logs/` trong thư mục dữ liệu. | Đã sửa (`6537b5c`) |
| S3 | Cao | Hiển thị tin nhắn chứa link khiến app tự gọi thẳng URL đó (lộ IP, xác nhận đã xem, có thể chạm địa chỉ LAN, chấp nhận `http://`, tải toàn bộ nội dung trước khi cắt). | `src/gui/messages.rs:1175-1193` → `src/chat.rs:1049-1068` | Tin nhắn đến chỉ dùng kết quả `parselink` phía máy chủ Zalo. Truy cập trực tiếp chỉ cho link người dùng tự gửi, chỉ `https`, chặn IP nội bộ, giới hạn dung lượng. | Đã sửa (`6537b5c`) |
| S4 | Cao | Release `v0.0.2` của upstream có file không do CI build: `latest.json` và exe `0.0.2` được upload tay; 9 file mang tên `0.0.1` nhưng SHA256 khác bản `v0.0.1`. Tiêu đề quảng cáo "Self-Updater" trong khi mã nguồn không có. | Trang Releases upstream (issue #12) | Không dùng binary của upstream. QTzalo chỉ phát hành file do CI build, kèm `SHA256SUMS.txt`. | Đã xử lý trong quy trình phát hành của QTzalo |
| S5 | Trung bình | Sinh số ngẫu nhiên bằng LCG/xorshift lấy seed theo thời gian (dễ đoán) cho IMEI/device-id, session cuộc gọi, salt mật khẩu, `zcid_ext`. | `src/param_cipher.rs:27`, `src/login.rs:40`, `src/call.rs:1592`, `src/call.rs:1614` | Dùng nguồn ngẫu nhiên của hệ điều hành (`getrandom`). | Đã sửa (`6537b5c`) |
| S6 | Trung bình | Media cuộc gọi: client khai `srtpMode: 0`. Socket UDP nhận gói từ mọi nguồn, không kiểm tra địa chỉ relay. Luồng H.264 từ phía bên kia đưa thẳng vào `ffmpeg` của hệ thống. | `src/call.rs:1416`, `src/call.rs:1788`, `src/call.rs:1862`, `src/media.rs:1543` | Lọc gói theo địa chỉ relay đã chọn. Khuyến nghị giữ `ffmpeg` luôn cập nhật. SRTP phụ thuộc giao thức Zalo. | Chưa sửa — cần thử với cuộc gọi thật trước khi lọc gói UDP (để phiên bản sau) |
| S7 | Trung bình | Mật khẩu qua tham số `--pass` lộ trong danh sách tiến trình và lịch sử shell. Ô nhập mật khẩu hiện chữ ra màn hình. `--pass-stdin` được nhắc nhưng không tồn tại. | `src/main.rs:176`, `src/ui.rs:42` | Bỏ `--pass`, nhập không hiện chữ, hỗ trợ `--pass-stdin`. | Đã sửa (`6537b5c`) |
| S8 | Trung bình | Token `zpw_sek` được gửi cho mọi URL WebSocket, kể cả `ws://` không mã hoá và địa chỉ đặt qua biến môi trường `ZCALL_WS`. HTTP thì có kiểm tra domain. | `src/socket.rs:271`, `src/socket.rs:394-398`, `src/session.rs:529` | Chỉ gửi cookie tới `wss://` thuộc domain Zalo. | Đã sửa (`6537b5c`) |
| S9 | Trung bình | Dependency: `tokio 1.54.0` (RUSTSEC-2026-0384, đã bị yank); `lru 0.12.5` unsound (qua ratatui 0.29); `audiopus_sys`, `paste`, `ttf-parser` không còn bảo trì; `clipboard-win 5.4.1` bị yank. | `Cargo.lock` | Nâng `tokio ≥ 1.54.1`. Theo dõi các crate còn lại. | Đã sửa một phần (`493f4ee`): tokio 1.54.1, clipboard-win 5.4.3. Bản 0.1.1 (`c4f1d19`): ratatui 0.30 / crossterm 0.29 bỏ `lru 0.12.5` và `paste`. Còn cảnh báo hết bảo trì: `audiopus_sys`, `ttf-parser` — theo dõi |
| S10 | Trung bình | CI cấp `contents: write` cho mọi job. Action chỉ ghim theo tag. `appimagetool` tải từ bản `continuous`, không kiểm tra checksum. Phiên bản bị ghi cứng ở nhiều nơi. | `.github/workflows/ci-release.yml` (bản cũ) | Quyền mặc định `read`, chỉ job release có `write`. Ghim action theo commit SHA. Bỏ AppImage. Lấy phiên bản từ `Cargo.toml`. Kiểm tra tag khớp phiên bản. | Đã sửa trong workflow mới |
| S11 | Thấp | Giải nén dữ liệu WebSocket không giới hạn kích thước (dữ liệu đến từ máy chủ Zalo qua TLS). | `src/socket.rs` (`inflate`) | Giới hạn kích thước sau giải nén. | Đã sửa (`6537b5c`) |
| S12 | Thấp | AES-CBC với IV bằng 0, khoá tham số ghi cứng, khoá AES-GCM gửi kèm trong cùng kết nối. | `src/crypto.rs`, `src/param_cipher.rs:15`, `src/socket.rs` | Đặc thù giao thức Zalo. Lớp bảo vệ thực tế là TLS (rustls, không tắt kiểm tra chứng chỉ). | Không sửa được phía client |
| S13 | Thấp | Thư mục `web-bundle/` chứa 3,9 MB JavaScript đã làm rối của Zalo, không dùng khi build (rủi ro bản quyền). | `web-bundle/` | Xoá khỏi repo. | Đã xoá |
| S14 | Thấp | Đường dẫn máy phát triển còn sót trong mã (`F:\CODE\Reverse-engineer\...`). | `src/call.rs:286` | Xoá. | Đã sửa (`6537b5c`) |
| S15 | Trung bình | (Tính năng mới 0.1.1) Tải tệp/ảnh/video từ tin nhắn về máy: URL và tên file do người gửi kiểm soát — rủi ro path traversal, ghi đè file, tên thiết bị Windows (`CON`, `COM1`…), ký tự đảo chiều (U+202E giả `exe` thành `pdf`), tải từ `http://` không mã hoá, tự mở file thực thi. | `src/download.rs`, `src/gui/worker.rs` (`DownloadAttachment`) | Chỉ `https` (`http` chỉ cho CDN Zalo), kiểm tra lại sau chuyển hướng, từ chối URL có user/pass; làm sạch tên file (bỏ `/ \ : * ? " < > \|`, ký tự điều khiển/bidi/zero-width, dấu chấm/khoảng trắng đầu-cuối, đổi tên thiết bị Windows, ≤150 ký tự); không ghi đè (`tên (1).ext`, giữ chỗ `.part` bằng `create_new`, đổi tên bằng hard link không thay thế); ghi theo luồng, giới hạn 2 GiB, kiểm tra đủ `Content-Length`; thư mục đích phải tuyệt đối; Mark-of-the-Web (`Zone.Identifier`, `ZoneId=3`, chỉ ghi scheme+host) trên Windows; nút "Mở" chỉ mở thư mục với `.exe .msi .bat .cmd .ps1 .js .lnk .jar .reg .iso …`. Có unit test + test end-to-end với server cục bộ. | Đã sửa (`a0f59d0`, `ebe8aef`) |
| S16 | Thấp | Ảnh dán từ clipboard được ghi vào `stickers_cache/` **tương đối** với thư mục đang đứng khi chạy app, và không bao giờ bị dọn. | `src/gui/messages.rs:443-448` | Ghi vào `paths::cache_dir()`; khi khởi động xoá `clipboard_*.png` cũ hơn 24 giờ (chỉ file thường, không theo symlink). | Đã sửa (`d877bba`, `6b39365`) |
**Điểm đã làm tốt:**
- SQL đều dùng truy vấn tham số hoá.
- Mở URL chỉ chấp nhận http/https và không đi qua shell.
- Không có secret trong lịch sử git.
- Tên file cache được băm (không bị path traversal).
- Bộ phân tích gói RTP kiểm tra biên.
- Chỉ có 3 khối `unsafe` (gọi Win32 API).

## 2. Lỗi chức năng

| # | Vấn đề | Issue upstream | Vị trí (tại `73849bd`) | Trạng thái |
|---|---|---|---|---|
| B1 | Lỗi mạng/HTTP bị coi là "phiên hết hạn" và xoá `session.json`. Đăng xuất còn xoá cả `zalo_history.db`. | #3, #21 | `src/session.rs:267`, `src/gui/worker.rs:419-421`, `src/gui/worker.rs:617`, `src/main.rs:146` | Đã sửa (`6537b5c`) |
| B2 | `zpw_service_map_v3` từ server ghi đè domain đã kiểm chứng, khiến danh sách bạn bè lỗi 404. Khi nạp phiên chỉ khôi phục 3/13 domain. | #3, #22 (PR #29) | `src/session.rs:385-406`, `src/session.rs:535-557`, `src/chat.rs` (`get_friends`) | Đã sửa (`6537b5c`, viết lại theo hướng của PR #29) |
| B3 | Khoá gọi thử nghiệm còn sót: chỉ gọi được liên hệ có tên chứa "hoang", "do thanh", "dtc". | — | `src/call.rs:84-100`, `src/call.rs:156-192` | Đã sửa (`6537b5c`) |
| B4 | Giờ tin nhắn hiển thị theo UTC (lệch 7 tiếng ở Việt Nam), trong khi một số chỗ lại cộng cứng UTC+7. | #4 | `src/chat.rs:467`, `src/chat.rs:3364`, `src/store.rs:278`, `src/ui.rs:51`, `src/gui/messages.rs:798` | Đã sửa (`9184156` + `6537b5c`, phần GUI ở bản giao diện mới) |
| B5 | Thiếu font tiếng Việt trên Fedora, không có font chữ Hán (CJK) dự phòng. | #5, #6 | `src/gui/theme.rs:533-546` | Đã sửa (`56f4d16`) |
| B6 | Nút "Đồng bộ tin nhắn" chỉ làm mới danh sách hội thoại nhưng thông báo như đã đồng bộ. | #8, #10, #19, #23 | `src/gui/modals.rs:73`, `src/gui/worker.rs:609` | Đã sửa phần hiển thị (`56f4d16`): đổi tên thành "Làm mới hội thoại", thông báo đúng sự thật. Đồng bộ toàn bộ lịch sử (#19) để phiên bản sau |
| B7 | Phiên bản bị ghi cứng `0.0.1` trong CI và bộ cài. | #2 (PR #18) | `.github/workflows/ci-release.yml`, `packaging/` | Đã sửa trong workflow mới |
| B8 | README ghi Rust 1.75+, thực tế cần **1.88**: Rust 1.87 bị cargo từ chối vì `cookie_store 0.22.1`, `darling 0.24.1`, `image 0.25.10`, `time 0.3.55`. | — | `README.md` | Đã sửa |
| B9 | `build.rs` dùng `#[cfg(target_os = "windows")]` (kiểm tra máy build, không phải máy đích) nên không nhúng icon khi build chéo. | — | `build.rs` | Đã sửa (đọc `CARGO_CFG_TARGET_OS`) |

## 3. Môi trường build đã kiểm chứng

| Phiên bản Rust | Kết quả |
|---|---|
| 1.87.0 | Thất bại: `rustc 1.87.0 is not supported by the following packages: cookie_store@0.22.1, darling@0.24.1 …` |
| 1.88.0 | `cargo check --locked` thành công |
| 1.97.0 (stable lúc rà soát) | `cargo build` + `cargo test` thành công (95 test qua, 1 bỏ qua) |
| 1.88.0 — bản QTzalo 0.1.1 | `cargo +1.88 check --locked --all-targets` thành công (sau khi nâng ratatui 0.30) |
| 1.97.0 — bản QTzalo 0.1.1 | `cargo test --locked`: 147 test qua, 1 bỏ qua; `scripts/check-windows.sh` sạch; `cargo audit`: 0 lỗ hổng, 2 cảnh báo hết bảo trì |
| 1.97.0 — bản QTzalo 0.1.0 | `cargo test --locked`: 123 test qua, 1 bỏ qua; `cargo check --target x86_64-pc-windows-gnu` sạch; CI Windows MSVC xanh |
