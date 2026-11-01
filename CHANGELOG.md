# Nhật ký thay đổi

Mọi thay đổi đáng chú ý của QTzalo được ghi tại đây. Định dạng theo [Keep a Changelog](https://keepachangelog.com/vi/1.1.0/), phiên bản theo [Semantic Versioning](https://semver.org/lang/vi/).

QTzalo là bản fork của [ctdozlite](https://github.com/ctdoteam1110/ctdozlite), tách ra từ commit `73849bd` (tag `v0.0.2` của upstream).

## [0.1.1] - 2026-11-01

### Thêm mới

- **Tải tệp, video và ảnh gốc về "Thư mục lưu file"**
  - Nút tải về trên thẻ tệp, thẻ video, ảnh trong khung chat (và trong menu chuột phải), cùng nút "Tải về" trên thanh công cụ trình xem ảnh.
  - Hiển thị tiến trình (phần trăm khi máy chủ báo dung lượng), trạng thái "Đã tải" với nút **Mở** / **Mở thư mục**, báo lỗi rõ ràng và nút **Thử lại**.
  - Tải xong hiện thông báo có nút **Mở** và **Mở thư mục**.
  - Tải theo luồng thẳng xuống đĩa, không giữ cả file trong RAM.
- **Quản lý cache ảnh** trong *Cài đặt → Dữ liệu & làm mới → Lưu trữ trên máy*: xem đường dẫn thư mục dữ liệu (nút "Mở thư mục dữ liệu"), dung lượng cache ảnh (tính ở luồng nền), nút **Xoá cache ảnh** có hộp xác nhận. Chỉ xoá `stickers_cache/`; phiên đăng nhập, lịch sử tin nhắn và cài đặt không bị đụng tới. Ảnh tự tải lại sau khi xoá.
- **Số thành viên nhóm thật** ở header khung chat ("N thành viên"), lấy từ trường `totalMember` của danh sách nhóm nếu máy chủ trả về; nếu không có vẫn hiện "Nhóm".

### Sửa lỗi

- Ảnh dán từ clipboard không còn bị ghi vào thư mục đang đứng khi chạy app; nay nằm trong cache của thư mục dữ liệu và được dọn sau 24 giờ.
- Nút **Thay đổi** thư mục lưu file dùng hộp chọn thư mục của hệ điều hành (trước đây phải chọn một file rồi lấy thư mục cha, không chọn được thư mục rỗng).
- Bố cục cửa sổ Cài đặt bị dàn ngang và tràn màn hình; nút "Làm mới hội thoại" mất chữ ở giao diện sáng.

### Bảo mật

- Tính năng tải về coi URL và tên file trong tin nhắn là dữ liệu không tin cậy (S15 trong `docs/SECURITY_AUDIT.md`):
  - chỉ tải qua `https://` (`http://` chỉ cho CDN Zalo), kiểm tra lại sau chuyển hướng;
  - làm sạch tên file (ký tự cấm, ký tự điều khiển và ký tự đảo chiều U+202E, tên thiết bị Windows như `CON`/`COM1`, độ dài ≤ 150 ký tự);
  - không bao giờ ghi đè file có sẵn (`tên (1).ext`), ghi vào `*.part` rồi mới đổi tên;
  - trên Windows gắn Mark-of-the-Web (`Zone.Identifier`, `ZoneId=3`) để SmartScreen/Office cảnh báo đúng;
  - không tự mở file thực thi/script (`.exe .msi .bat .cmd .ps1 .js .lnk .jar .reg .iso`…): nút **Mở** chỉ mở thư mục chứa file.
- Dependency: nâng `ratatui` 0.30 và `crossterm` 0.29, loại bỏ `lru 0.12.5` (unsound, RUSTSEC-2026-0002 / RUSTSEC-2026-0253) và `paste` (hết bảo trì). Cập nhật patch: `log`, `memmap2`, `rustix`, `smallvec`, `unicode-width`, `webbrowser`, `webpki-roots`, `zune-core`. `cargo audit`: 0 lỗ hổng, còn 2 cảnh báo hết bảo trì (`audiopus_sys`, `ttf-parser`).

### Phát triển

- Module mới `src/download.rs` (chính sách tải tách khỏi I/O, có unit test và test end-to-end với server cục bộ).
- Biến môi trường cho chế độ `--demo`: `QTZALO_DEMO_DOWNLOADS=1`, `QTZALO_DEMO_SETTINGS=<tab>`, `QTZALO_DEMO_CONFIRM_CLEAR=1`.

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

- **Thiết kế lại toàn bộ theo bố cục 3 cột**, giao diện sáng mặc định, tông xanh nhạt và bo góc mềm. Giao diện tối vẫn chọn được trong Cài đặt.
- **Thanh điều hướng trái** gồm 3 nhóm:
  - Hộp thư: Tin nhắn, Khác, Chưa đọc, Người lạ, Trò chuyện ẩn.
  - Lưu trữ: Cloud của tôi, zCloud kèm thanh dung lượng dữ liệu trên máy.
  - Danh bạ: Bạn bè, Nhóm và cộng đồng, Lời mời kết bạn.
  - Có badge đếm, chân trang hồ sơ kèm nút cài đặt. Mục chưa có tính năng hiện "Sắp có".
- **Danh sách hội thoại**
  - Avatar chữ cái nhiều màu, badge chưa đọc, hàng đang chọn tô xanh nhạt.
  - Ô tìm kiếm có phím tắt Ctrl+K (⌘K trên macOS).
  - Giờ hiển thị kiểu HH:MM / Hôm qua / T2…CN / dd/mm.
- **Thu gọn cột:**
  - Nút thu gọn đưa thanh điều hướng về dạng chỉ còn icon và danh sách hội thoại về dạng chỉ còn avatar.
  - Có hiệu ứng chuyển động, trạng thái được lưu lại.
  - Cửa sổ hẹp hơn 1100 px tự thu gọn thanh điều hướng; hẹp hơn 760 px thu gọn cả hai cột.
- **Khung chat**
  - Nền gradient nhẹ, header có avatar, tên và số thành viên.
  - Chip ngày "Hôm nay" / "Thứ Ba, 27/10".
  - Bong bóng gửi đi màu xanh đặc, nhận vào màu sáng có tên người gửi.
  - Chip reaction, khối trích dẫn, thẻ tệp, chỉ báo "đang nhập".
  - Khung soạn tin có nút (+) mở menu ảnh/tệp/dán clipboard, nút sticker, emoji và nút gửi tròn.
- Cửa sổ mặc định 1280×800, tối thiểu 480×360. Thanh tiêu đề hiện "QTzalo" và phiên bản.
- **Font chữ (#5, #6)**
  - Tự dò font hệ thống theo thứ tự: font Latin hỗ trợ tiếng Việt, font chữ Hán (CJK, đọc được file `.ttc`), rồi font emoji/ký hiệu.
  - Hỗ trợ đường dẫn font của Fedora, Debian/Ubuntu, Arch, macOS và Windows (Segoe UI, Microsoft YaHei, Segoe UI Emoji).
- Hiển thị giờ trên giao diện dùng múi giờ của máy, bỏ phép cộng cứng UTC+7 (#4).
- **Đổi nút "Đồng bộ tin nhắn" thành "Làm mới hội thoại" (#23)**
  - Thông báo đúng việc nút thực sự làm và không còn hiện khi app tự làm mới lúc khởi động.
  - Đồng bộ toàn bộ lịch sử (#19) để phiên bản sau.
- Chế độ `--demo`: giao diện với dữ liệu mẫu offline, không kết nối mạng, không đọc hay ghi dữ liệu thật. Dùng để chụp ảnh và kiểm thử giao diện.
- Sửa lỗi crash `clamp(min > max)` khi khung chat quá hẹp, vạch đen giữa các panel, và lỗi theme hệ thống ghi đè bảng màu.

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

[0.1.1]: https://github.com/quangthoai1985/QTzalo/releases/tag/v0.1.1
[0.1.0]: https://github.com/quangthoai1985/QTzalo/releases/tag/v0.1.0
