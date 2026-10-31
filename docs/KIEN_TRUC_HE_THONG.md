# Kiến trúc hệ thống ctdozlite

Tài liệu mô tả cách phân tách luồng xử lý, tổ chức module giao diện và cơ chế lưu trữ cục bộ của `ctdozlite`.

---

## 1. Sơ đồ tổng thể

```text
┌────────────────────────────────────────────────────────────────────────┐
│                           TẦNG GIAO DIỆN (UI)                          │
│                                                                        │
│   ┌───────────────────────────────┐  ┌─────────────────────────────┐   │
│   │  GUI (src/gui.rs + src/gui/*) │  │     TUI (src/tui.rs)        │   │
│   │  • eframe / egui              │  │  • ratatui / crossterm      │   │
│   │  • Event-driven repaint       │  │  • 30ms terminal poll loop  │   │
│   └───────────────┬───────────────┘  └──────────────┬──────────────┘   │
└───────────────────┼─────────────────────────────────┼──────────────────┘
                    │ WorkerCmd (mpsc)                │
                    ▼                                 ▼
┌────────────────────────────────────────────────────────────────────────┐
│                     LUỒNG NỀN (TOKIO ASYNC WORKER)                     │
│                                                                        │
│   • WebSocket loop (`zpw_ws`): nhận tin nhắn & sự kiện VoIP tức thì    │
│   • HTTP Poller (`get_last_msgs` / `lp_control`): đồng bộ dự phòng     │
│   • Sticker Engine: gọi song song suggest + search & prefetch ảnh      │
│   • CallBridge (`src/call.rs`): quản lý báo hiệu & luồng RTP/RTCP      │
└───────────────────┬─────────────────────────────────┬──────────────────┘
                    │                                 │
                    ▼                                 ▼
┌───────────────────────────────────┐  ┌─────────────────────────────────┐
│        LƯU TRỮ CỤC BỘ             │  │      MẬT MÃ & PHIÊN MẠNG        │
│                                   │  │                                 │
│ • threads_cache.json (snapshot)   │  │ • ParamCipher v2 & SignKey MD5  │
│ • zalo_history.db (SQLite)        │  │ • AES-128-CBC / AES-256-CBC     │
│ • stickers_cache/ (WebP/PNG)      │  │ • Cookie jar đa domain zpw_sek  │
└───────────────────────────────────┘  └─────────────────────────────────┘
```

---

## 2. Tổ chức module GUI (`src/gui/`)

Để dễ bảo trì, phần giao diện đồ họa được chia thành các module con theo trách nhiệm:

- `src/gui.rs`: Khởi tạo `ZaloGuiApp`, quản lý trạng thái ứng dụng, xử lý hàng đợi `GuiEvent`, thanh tiêu đề cửa sổ và màn hình đăng nhập.
- `src/gui/theme.rs`: Bảng màu `ThemePalette` (Dark/Light), bộ icon vector `lucide`, vẽ avatar và nạp font tiếng Việt của hệ điều hành.
- `src/gui/stickers.rs`: Bảng chọn nhãn dán (`StickerPicker`), thuật toán gợi ý nhãn dán theo thời gian thực khi gõ (`resolve_contextual_sticker_suggestions`) và thanh popover gợi ý nổi trên ô nhập.
- `src/gui/worker.rs`: Định nghĩa `WorkerCmd`, luồng nền Tokio (`spawn_session_worker`) và bộ tải trước ảnh thumbnail nhãn dán (`preload_disk_sticker_thumbnails`, `prefetch_missing_sticker_thumbnails`).
- `src/gui/workspace.rs`: Bố cục 3 cột gồm thanh điều hướng, danh sách hội thoại, vùng khung chat chính và ngăn thông tin chi tiết bên phải.
- `src/gui/messages.rs`: Hiển thị bong bóng tin nhắn, nhãn dán, thẻ cuộc gọi, trích dẫn, cảm xúc (reaction), menu chuột phải và xử lý dán ảnh từ clipboard.
- `src/gui/modals.rs`: Các cửa sổ phụ gồm cài đặt, cuộc gọi thoại/video, xem ảnh phóng to, chuyển tiếp tin nhắn, tìm kiếm theo số điện thoại và Dev Inspector.

---

## 3. Cơ chế đồng bộ bất đồng bộ

### 3.1. Tách biệt luồng vẽ giao diện và luồng mạng
- Luồng giao diện (UI thread) không bao giờ gọi trực tiếp các hàm mạng chặn (blocking). Mọi thao tác mạng được gửi qua kênh `tokio::sync::mpsc::unbounded_channel` dưới dạng `WorkerCmd`.
- Luồng worker chạy trên một Tokio runtime 2 worker threads, xử lý kết nối WebSocket `zpw_ws`, các truy vấn REST API và giải mã ảnh trên nền. Khi có kết quả, worker gửi `GuiEvent` về UI kèm lệnh `egui_ctx.request_repaint()`.

### 3.2. Gợi ý nhãn dán & Prefetch ảnh song song
- Khi người dùng gõ từ khóa trong ô soạn thảo (sau khoảng trễ debounce 145ms cho chữ cái hoặc 0ms cho biểu tượng cảm xúc/từ khóa đã cache), UI gửi lệnh `WorkerCmd::SuggestComposerStickers`.
- Worker chuẩn hóa chuỗi truy vấn tiếng Việt (sinh cả dạng có dấu, không dấu và từ đồng nghĩa/viết tắt), sau đó gọi đồng thời hai API `/api/message/sticker/suggest/stickers` (`cmd 11741`) và `/api/message/sticker/search` (`cmd 11747`).
- Ngay khi nhận danh sách ID nhãn dán, worker nạp trước các file ảnh đã có trong `stickers_cache/` lên bộ nhớ texture trước khi gửi `GuiEvent::ComposerStickerSuggestions`, đồng thời tải song song các ảnh còn thiếu qua HTTP để tránh hiện tượng nhấp nháy khung chờ.

---

## 4. Lưu trữ dữ liệu 2 tầng

1. **Snapshot JSON (`threads_cache.json`)**: Lưu danh sách hội thoại và các tin nhắn gần nhất để hiển thị ngay khi khởi động ứng dụng.
2. **SQLite (`zalo_history.db`)**: Lưu trữ lâu dài toàn bộ tin nhắn theo bảng `messages(msg_id, thread_id, sender_id, sender_name, text, msg_type, ts, is_outgoing, raw_json)` có đánh chỉ mục trên `(thread_id, ts)`. Khi đồng bộ tin nhắn mới, hệ thống khử trùng theo `msg_id` và theo cặp `(text, is_outgoing, ts)` trong biên độ 5 giây.
