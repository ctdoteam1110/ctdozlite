# Changelog

Mọi thay đổi đáng chú ý của `ctdozlite` được ghi nhận tại tệp này theo chuẩn [Keep a Changelog](https://keepachangelog.com/vi/1.1.0/) và [Semantic Versioning](https://semver.org/).
Xem thêm quy ước tại [ADR 0003 — Chính sách Changelog & Release Notes](docs/adr/0003-changelog-policy.md).

## [Unreleased]

## [0.0.5] - 2026-11-02

### Fixed (Sửa lỗi)
- **Sửa triệt để lỗi gộp nhầm/nhảy sai hội thoại khi tên Zalo trùng nhau ([#63](https://github.com/ctdoteam1110/ctdozlite/issues/63)):** Tách biệt hoàn toàn định danh theo `thread_id` thực tế trong `threads_represent_same_contact` (`src/chat.rs`), khôi phục lựa chọn hội thoại theo `t.id == prev.id` (`src/gui.rs`), giới hạn ánh xạ tên trong SQLite `hydrate_threads` (`src/store.rs`) chỉ cho `"Cloud của tôi"`, và ưu tiên `partner_id` khi tra cứu avatar cuộc gọi.
- **Sửa lỗi khởi tạo & kiểm tra gói cài đặt macOS native helpers ([#58](https://github.com/ctdoteam1110/ctdozlite/issues/58), [#20](https://github.com/ctdoteam1110/ctdozlite/issues/20)):** Xác thực kiến trúc `arm64`/`x86_64`/Universal và chữ ký ad-hoc cho `ctdozlite.app` trước khi đóng gói `.dmg`.
- **Mạng & Failover đa máy chủ tự động ([#27](https://github.com/ctdoteam1110/ctdozlite/issues/27)):** Tự động chuyển tiếp qua danh sách máy chủ dự phòng cho từng dịch vụ (`friend`, `group`, `group_poll`, `chat`, `msg`, `conversation`, `profile`) khi gặp lỗi kết nối hoặc HTTP 404/5xx.
- **Làm mới hội thoại minh bạch ([#23](https://github.com/ctdoteam1110/ctdozlite/issues/23)):** Đổi nhãn nút "Đã đồng bộ đám mây" trong hồ sơ thành "Làm mới hội thoại" và hiển thị chính xác số hội thoại/tin nhắn mới hoặc thông báo lỗi mạng thực tế.

### Added (Tính năng mới & Cập nhật)
- **Tự động kiểm tra, thông báo & cập nhật trực tiếp qua GitHub Releases API:** Tự động truy vấn `https://api.github.com/repos/ctdoteam1110/ctdozlite/releases/latest` khi khởi động và định kỳ mỗi 30 phút, gửi thông báo hệ điều hành (macOS Notification Center / Linux `notify-send` / Windows PowerShell toast), hiển thị thanh thông báo nổi và hộp thoại chi tiết danh sách sửa lỗi/cập nhật kèm nút **Cập nhật tự động ngay** (xác thực `SHA256SUMS.txt` + chữ ký số).
- **Hẹn giờ gửi tin nhắn & Nhắc hẹn ([#38](https://github.com/ctdoteam1110/ctdozlite/issues/38), [#24](https://github.com/ctdoteam1110/ctdozlite/issues/24)):** Bổ sung bảng `scheduled_messages` trong SQLite, bộ điều phối nền kiểm tra mỗi 2 giây trong worker, nút `🕒` cạnh ô nhập tin nhắn và hộp thoại quản lý tin nhắn hẹn giờ (`+15m`, `+1h`, `18:00 hôm nay`, `08:00 ngày mai` hoặc `YYYY-MM-DD HH:MM`).
- **REST API, MCP Server & Auto-Reply ([#35](https://github.com/ctdoteam1110/ctdozlite/issues/35)):**
  - Máy chủ MCP chuẩn JSON-RPC 2.0 qua `--mcp` (stdio) và `POST /api/v1/mcp` (`list_threads`, `get_messages`, `search_messages`, `send_message`, `schedule_message`, `list_scheduled`, `cancel_scheduled`, `list_autoreply_rules`).
  - Máy chủ HTTP REST API cục bộ (`127.0.0.1`) qua `--api-port <PORT>` hoặc `CTDOZLITE_API_PORT`.
  - Bộ quy tắc tự động trả lời (`autoreply_rules.json`) quản lý trực tiếp trong **Cài đặt → Tiện ích**.
- **Nhập (Migrate) lịch sử tin nhắn từ thư mục/máy khác ([#9](https://github.com/ctdoteam1110/ctdozlite/issues/9)):** Hỗ trợ gộp dữ liệu lịch sử từ file `.db`/`.sqlite`, `.json`/`.jsonl` hoặc toàn bộ thư mục vào `zalo_history.db` một cách idempotent.
- **Tùy biến Theme & Sticker Pack bằng JSON ([#7](https://github.com/ctdoteam1110/ctdozlite/issues/7)):** Hỗ trợ xuất/nạp bảng màu `theme.json` và bộ nhãn dán `custom_stickers.json` ngay trong **Cài đặt → Giao diện**.
- **Menu chuột phải quản lý hội thoại ([#5](https://github.com/ctdoteam1110/ctdozlite/issues/5), [#24](https://github.com/ctdoteam1110/ctdozlite/issues/24)):** Hỗ trợ Ghim/Bỏ ghim, Tắt/Bật thông báo, Ẩn/Hiện hội thoại, Hẹn giờ gửi tin nhắn và Xoá lịch sử cuộc trò chuyện.
- **Đóng gói cục bộ ([#15](https://github.com/ctdoteam1110/ctdozlite/issues/15)), Phân loại Release Notes ([#14](https://github.com/ctdoteam1110/ctdozlite/issues/14)) & Tối ưu CI ([#17](https://github.com/ctdoteam1110/ctdozlite/issues/17), [#13](https://github.com/ctdoteam1110/ctdozlite/issues/13), [#12](https://github.com/ctdoteam1110/ctdozlite/issues/12), [#10](https://github.com/ctdoteam1110/ctdozlite/issues/10)):** Thêm `packaging/package-local.sh`, `packaging/package-local.ps1`, `.github/release.yml`, manifest `latest.json` kèm SHA-256 và bộ test giao thức giải mã (`#19`, `#16`).

## [0.0.3] - 2026-04-19

### Added
- Đồng bộ lịch sử phân trang có lưu trạng thái (`src/history_sync.rs`) và giải mã gói sao lưu lịch sử Zalo PC (`src/history_backup.rs`).
- Trình tự cập nhật an toàn có kiểm tra mã băm SHA-256 và chữ ký tệp thực thi (`src/updater.rs`).
- Hỗ trợ tùy chọn `--data-dir` và biến môi trường `CTDOZLITE_DATA_DIR` (`src/paths.rs`).
