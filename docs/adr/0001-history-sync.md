# ADR 0001: Kiến trúc đồng bộ lịch sử tin nhắn (History Sync & Backup Import)

- **Trạng thái:** Đã chấp nhận (Accepted)
- **Issues liên quan:** [#10](https://github.com/ctdoteam1110/ctdozlite/issues/10), [#19](https://github.com/ctdoteam1110/ctdozlite/issues/19), [#9](https://github.com/ctdoteam1110/ctdozlite/issues/9)

## Bối cảnh

Zalo Web/PC sử dụng kết hợp 3 nguồn dữ liệu lịch sử:
1. Hàng đợi tin nhắn thời gian thực (`poll_new_messages` / WebSocket) chỉ trả về các tin nhắn phát sinh từ thời điểm phiên kết nối hoặc tối đa ~50 tin gần nhất.
2. API phân trang lịch sử theo từng hội thoại nhóm và Cloud (`get_group_chat_history`, `get_cloud_history`).
3. Luồng đồng bộ lịch sử mã hóa/nén (`HistoryCodec` Zstandard + AES-CBC/GCM) hoặc cơ sở dữ liệu cục bộ trên máy cũ.

## Quyết định

Chúng tôi áp dụng kiến trúc đồng bộ lịch sử 3 tầng:
1. **Đồng bộ phân trang có lưu con trỏ (`src/history_sync.rs`):** Lưu bảng `history_sync_cursors` trong SQLite (`zalo_history.db`) để nối tiếp quá trình tải lịch sử nhóm và Cloud qua nhiều lần mở ứng dụng mà không tải trùng.
2. **Giải mã gói sao lưu lịch sử (`src/history_backup.rs`):** Hỗ trợ giải mã và giải nén payload lịch sử (`raw`, `deflate`, `gzip`, `zstd`) và trích xuất tin nhắn vào SQLite.
3. **Di chuyển lịch sử cục bộ (`store::import_history_from_path`):** Cho phép người dùng nhập trực tiếp tệp `.db`/`.sqlite`, `.json`/`.jsonl` hoặc cả thư mục dữ liệu từ máy khác vào `zalo_history.db` một cách idempotent (`INSERT OR IGNORE` theo `(thread_id, msg_id)`).
