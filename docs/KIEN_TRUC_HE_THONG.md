# Kiến Trúc Hệ Thống ctdozlite (Zalo Native Client)

Tài liệu này mô tả chi tiết kiến trúc phần mềm, mô hình xử lý đa luồng, cơ chế đồng bộ và hệ thống lưu trữ dữ liệu của **ctdozlite**.

---

## 1. Sơ đồ khối tổng thể (High-level Architecture)

```
┌────────────────────────────────────────────────────────────────────────┐
│                        TẦNG GIAO DIỆN (UI LAYER)                       │
│                                                                        │
│   ┌───────────────────────────┐      ┌─────────────────────────────┐   │
│   │   GUI (eframe / egui)     │      │   TUI (ratatui / crossterm) │   │
│   │  • 60 FPS Glow Render     │      │  • 30ms Event Loop          │   │
│   │  • Segoe UI Custom Font   │      │  • Flat ANSI Borders        │   │
│   │  • Dev Inspector Panel    │      │  • Keystroke Auto-focus     │   │
│   └─────────────┬─────────────┘      └──────────────┬──────────────┘   │
└─────────────────┼───────────────────────────────────┼──────────────────┘
                  │ mpsc::channel                     │ mpsc::channel
                  ▼                                   ▼
┌────────────────────────────────────────────────────────────────────────┐
│                    TẦNG XỬ LÝ NỀN (BACKGROUND WORKER)                  │
│                                                                        │
│   ┌────────────────────────────────────────────────────────────────┐   │
│   │             Tokio Multi-threaded Runtime (Non-blocking)        │   │
│   │  • WorkerCmd Receiver: Xử lý RequestQr, SendMessage, Logout    │   │
│   │  • Background Poller (3s): Gọi get_last_msgs chống lỗi 429     │   │
│   │  • GuiEvent / BgEvent Sender: Bắn dữ liệu về UI không delay   │   │
│   └────────────────────────────────┬───────────────────────────────┘   │
└────────────────────────────────────┼───────────────────────────────────┘
                                     │
         ┌───────────────────────────┴───────────────────────────┐
         ▼                                                       ▼
┌───────────────────────────────────┐   ┌────────────────────────────────┐
│   TẦNG DỮ LIỆU & LƯU TRỮ (STORE)  │   │   TẦNG MẬT MÃ & PHIÊN (CRYPTO) │
│                                   │   │                                │
│ • JSON Cache (threads_cache.json) │   │ • ParamCipher v2 (zcid, AES)   │
│   - Snapshot nạp nhanh < 5ms      │   │ • PasswordCipher (loginPassv2) │
│ • SQLite Database (zalo_history)  │   │ • SignKey Generator (MD5)      │
│   - Index (thread_id, ts)         │   │ • AES-128-CBC / AES-256-CBC    │
│   - Deduplication 2 tầng          │   │ • Session Jar & Domain Router  │
└───────────────────────────────────┘   └────────────────────────────────┘
```

---

## 2. Mô hình Xử lý Bất đồng bộ (Non-blocking Asynchronous Runtime)

Một trong những hạn chế lớn nhất của phiên bản cũ là việc gọi mạng đồng bộ ngay trong vòng lặp vẽ giao diện, dẫn đến hiện tượng đứng hình (freeze) khi mạng chậm hoặc khi đang chờ quét QR.

### 2.1. Phân tách Luồng Giao diện và Luồng Mạng
- **Luồng chính (UI Thread):** Chỉ thực hiện render khung hình và lắng nghe sự kiện chuột/bàn phím. Tốc độ làm tươi đạt 60 FPS (GUI) hoặc 30ms (TUI).
- **Luồng nền (Session Worker):** Chạy trên một Tokio Multi-threaded Runtime chuyên biệt. Mọi tác vụ tốn thời gian (HTTPS requests, long-polling QR 180 giây, nạp danh bạ 20.000 người) đều thực thi tại đây.
- **Kênh giao tiếp hai chiều (`tokio::sync::mpsc`):**
  - `WorkerCmd`: Gửi lệnh từ UI xuống Worker (`RequestQr`, `RefreshThreads`, `PollMessages`, `SendMessage`, `Logout`).
  - `GuiEvent`: Bắn kết quả từ Worker lên UI (`QrReady`, `QrScanned`, `LoginSuccess`, `ThreadsUpdated`, `MessageSent`, `LoggedOut`).

### 2.2. Cơ chế Polling chống giật (Jank-free Polling)
- Thay vì gọi toàn bộ danh bạ `getfriends` mỗi chu kỳ (vốn gây quá tải và bị máy chủ Zalo chặn `HTTP 429 Too Many Requests`), ứng dụng chỉ gọi `get_last_msgs` với `threadIdLocalMsgId = "{}"` mỗi 3 giây.
- Sử dụng cờ nguyên tử `IS_POLLING` để ngăn chặn việc gửi request mới khi request cũ chưa kịp phản hồi khi mạng lag.

---

## 3. Kiến trúc Lưu trữ Dữ liệu 2 Tầng (Two-tier Storage)

Để giải quyết triệt để vấn đề mất tin nhắn và tốc độ khởi động, hệ thống sử dụng kiến trúc lưu trữ 2 tầng:

### Tầng 1: Snapshot Cache (`threads_cache.json`)
- **Mục đích:** Lưu trữ trạng thái xem trước tức thì của danh sách hội thoại và các tin nhắn gần nhất.
- **Tốc độ:** Khởi động ứng dụng nạp file này trong thời gian dưới 5ms, người dùng thấy ngay danh bạ và tin nhắn trước khi máy chủ Zalo kịp phản hồi.

### Tầng 2: Cơ sở Dữ liệu Quan hệ (`zalo_history.db` - SQLite)
- **Mục đích:** Lưu trữ lịch sử tin nhắn tích lũy vĩnh viễn, hỗ trợ truy vấn và tìm kiếm tin nhắn cũ.
- **Schema:**
  ```sql
  CREATE TABLE IF NOT EXISTS messages (
      msg_id TEXT PRIMARY KEY,
      thread_id TEXT NOT NULL,
      sender_id TEXT,
      sender_name TEXT,
      text TEXT NOT NULL,
      msg_type TEXT NOT NULL,
      ts INTEGER NOT NULL,
      is_outgoing INTEGER NOT NULL,
      raw_json TEXT
  );
  CREATE INDEX IF NOT EXISTS idx_thread_ts ON messages(thread_id, ts);
  ```
- **Thuật toán Deduplication 2 lớp:**
  1. Khử trùng tuyệt đối theo `msg_id`.
  2. Khử trùng theo bộ `(text, is_outgoing, ts)` với sai số `|ts1 - ts2| < 5000 ms` để bảo vệ các tin nhắn được gửi từ client chưa kịp nhận ID chính thức từ server.

---

## 4. Tách bạch Chế độ Production và Dev Mode

Hệ thống được thiết kế theo nguyên tắc an toàn thông tin nghiêm ngặt:

- **Production Mode (Mặc định):**
  - Badge trạng thái màu xanh `[PRODUCTION]`.
  - Toàn bộ cơ chế log mạng (`logger::log_request`) lập tức return mà không cấp phát bộ nhớ.
  - Tuyệt đối không lưu URL thô, token, khóa bí mật hay payload giải mã vào bộ nhớ RAM.
- **Dev Mode (Kích hoạt qua `--dev`):**
  - Badge trạng thái màu vàng `[DEV MODE]`.
  - Bật ngăn kéo Dev Inspector ở đáy màn hình.
  - Bắt toàn bộ lưu lượng request/response, đo đạc độ trễ mạng chính xác đến mili-giây, hiển thị payload giải mã và chạy máy quét chẩn đoán mã lỗi tự động.
