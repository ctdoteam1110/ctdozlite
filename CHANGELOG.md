# Changelog

Mọi thay đổi đáng chú ý của `ctdozlite` được ghi nhận tại tệp này theo chuẩn [Keep a Changelog](https://keepachangelog.com/vi/1.1.0/) và [Semantic Versioning](https://semver.org/).
Xem thêm quy ước tại [ADR 0003 — Chính sách Changelog & Release Notes](docs/adr/0003-changelog-policy.md).

## [Unreleased]

### Added
- **Hẹn giờ gửi tin nhắn & Nhắc hẹn (`#38`, `#24`):** Bổ sung bảng `scheduled_messages` trong SQLite, bộ điều phối nền kiểm tra mỗi 2 giây trong worker, nút `🕒` cạnh ô nhập tin nhắn và hộp thoại quản lý tin nhắn hẹn giờ (hỗ trợ mốc nhanh `+15m`, `+1h`, `18:00 hôm nay`, `08:00 ngày mai` hoặc nhập thời gian tùy chỉnh).
- **REST API, MCP Server & Auto-Reply (`#35`):**
  - Máy chủ MCP chuẩn JSON-RPC 2.0 qua `--mcp` (stdio) và `POST /api/v1/mcp` với đầy đủ công cụ (`list_threads`, `get_messages`, `search_messages`, `send_message`, `schedule_message`, `list_scheduled`, `cancel_scheduled`, `list_autoreply_rules`).
  - Máy chủ HTTP REST API cục bộ (`127.0.0.1`) qua `--api-port <PORT>` hoặc `CTDOZLITE_API_PORT`.
  - Bộ quy tắc tự động trả lời (`autoreply_rules.json`) quản lý trực tiếp trong **Cài đặt → Tiện ích**.
- **Nhập (Migrate) lịch sử tin nhắn từ thư mục/máy khác (`#9`):** Hỗ trợ gộp dữ liệu lịch sử từ file `.db`/`.sqlite`, `.json`/`.jsonl` hoặc toàn bộ thư mục vào `zalo_history.db` một cách idempotent.
- **Tùy biến Theme & Sticker Pack bằng JSON (`#7`):** Hỗ trợ xuất/nạp bảng màu `theme.json` và bộ nhãn dán `custom_stickers.json` ngay trong **Cài đặt → Giao diện**.
- **Menu chuột phải quản lý hội thoại (`#5`, `#24`):** Hỗ trợ Ghim/Bỏ ghim, Tắt/Bật thông báo, Ẩn/Hiện hội thoại, Hẹn giờ gửi tin nhắn và Xoá lịch sử cuộc trò chuyện.
- **Đóng gói cục bộ (`#15`) & Phân loại Release Notes (`#14`):** Thêm `packaging/package-local.sh`, `packaging/package-local.ps1` và `.github/release.yml`.

### Changed
- **Mạng & Failover đa máy chủ (`#27`):** Tự động thử lần lượt qua danh sách máy chủ ứng viên cho mỗi dịch vụ (`friend`, `group`, `group_poll`, `chat`, `msg`, `conversation`, `profile`) khi gặp lỗi kết nối hoặc HTTP 404/5xx.
- **Làm mới hội thoại minh bạch (`#23`):** Đổi nhãn nút "Đã đồng bộ đám mây" trong hồ sơ thành "Làm mới hội thoại" và báo cáo trung thực số hội thoại/tin nhắn mới hoặc lỗi mạng.
- **Tối ưu CI trên Pull Request (`#17`, `#13`, `#12`):** Tách job `test` chạy nhanh trên PR, chỉ đóng gói toàn bộ 9 bộ cài đa nền tảng khi push lên `main`, tag `v*`, `workflow_dispatch` hoặc PR gắn nhãn `ci:full-package`; xuất `latest.json` kèm danh sách `assets` và mã băm SHA-256.

## [0.0.3] - 2026-04-19

### Added
- Đồng bộ lịch sử phân trang có lưu trạng thái (`src/history_sync.rs`) và giải mã gói sao lưu lịch sử Zalo PC (`src/history_backup.rs`).
- Trình tự cập nhật an toàn có kiểm tra mã băm SHA-256 và chữ ký tệp thực thi (`src/updater.rs`).
- Hỗ trợ tùy chọn `--data-dir` và biến môi trường `CTDOZLITE_DATA_DIR` (`src/paths.rs`).
