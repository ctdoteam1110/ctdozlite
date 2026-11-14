# BẢN GHI THAY ĐỔI CHI TIẾT (DIFF & UPDATES)
**Dự án:** ZLite (Native Rust Zalo PC Client)  
**So sánh giữa:** Bản phát hành cũ (`v0.1.1` - commit `7ed3408`) và **Bản cập nhật hiện tại**  
**Ngày cập nhật:** 14/11/2026  

---

## 📌 TỔNG QUAN CẬP NHẬT

Bản cập nhật này tập trung giải quyết toàn diện các lỗi hiển thị UI/UX, tối ưu hóa trải nghiệm tương tác với tin nhắn media (Video, Tập tin đính kèm), nâng cấp thanh soạn thảo tin nhắn Composer, hoàn thiện hệ thống đồng bộ lịch sử tin nhắn Protobuf Sync V2 theo chuẩn Zalo PC và sửa triệt để lỗi phát video in-app.

### Thống kê thay đổi (Git Diff Stat)
```text
17 files changed, 6456 insertions(+), 2041 deletions(-)
```

---

## 🚀 CHI TIẾT CÁC CẢI TIẾN THEO TỪNG MODULE

### 1. Giao diện Tin nhắn & Thẻ đính kèm (`src/gui/messages.rs`)

* **Thiết kế lại Thẻ tập tin đính kèm (File Attachment Card):**
  - **Bản cũ:** Tên file, phần mở rộng và 2 nút bấm `[ Mở ]` và `[ Lưu về máy ]` bị nén chật hẹp, nút bấm có kích thước lệch nhau và dính sát vào dòng thời gian (`16:03 · Đã nhận`). Với các file không có phần mở rộng (như token mã hóa), hệ thống tự động in hoa toàn bộ chuỗi token dài làm extension (`Tập tin · 2AOBORCN...`).
  - **Bản mới:**
    - Bổ sung **Icon Badge 40x40px** bên trái với nền mờ bo góc 8px và màu sắc phân loại chuyên biệt theo từng định dạng:
      - 📄 PDF: Đỏ (`#EB5757`)
      - 📝 Word: Xanh dương (`#2D78F0`)
      - 📊 Excel: Xanh lá (`#27AE60`)
      - 📈 PowerPoint: Cam (`#F2994A`)
      - 📦 Archive (`.rar`, `.zip`, `.7z`): Vàng hổ phách (`#F2C94C`)
      - 📎 Tập tin khác: Primary Accent.
    - Cột thông tin bên phải hiển thị tên file chữ đậm 13.5pt có tự động xuống dòng và tooltip khi hover xem đầy đủ tên dài.
    - Logic trích xuất phần mở rộng thông minh: Chỉ hiển thị định dạng khi có dấu chấm thực tế, không bao giờ in mã băm.
    - Bộ nút bấm đồng bộ: `[ Mở ]` (Primary, chiều rộng tối thiểu 64px, cao 28px) và `[ Lưu về máy ]` (Secondary viền mảnh tinh tế, chiều rộng tối thiểu 96px, cao 28px) cùng bo góc 6px.
    - Nới rộng khoảng cách đệm trên dưới: Đảm bảo giữa nút bấm và dòng thời gian footer cách nhau tối thiểu 12–14px, không bao giờ bị dính đè lên nhau.
    - Tăng chiều rộng tối thiểu thẻ file từ `240px` lên `280px` (tối đa `380px`), tên file dài hiển thị thoáng đãng.

* **Sửa lỗi mở đường dẫn file cục bộ (`open_external_url_with_monitor`):**
  - **Bản cũ:** Hàm mở liên kết chỉ xử lý link web (`http://`, `https://`). Khi nhận đường dẫn file cục bộ (ví dụ file video tải về máy `C:\Users\...\zlite_video_xxx.mp4`), hàm tự động thêm tiền tố `https://` thành `https://C:\Users\...` khiến Windows API `ShellExecuteW` thất bại hoàn toàn.
  - **Bản mới:** Bổ sung cơ chế phát hiện đường dẫn cục bộ (`file://`, ký tự ổ đĩa `C:\...`, UNC path `\\...`, đường dẫn Unix `/...` hoặc file tồn tại trên ổ cứng), truyền trực tiếp đường dẫn sạch vào Windows API với thao tác `"open"` để mở bằng ứng dụng mặc định của hệ điều hành.

* **Nhận diện và hiển thị Video Zalo:**
  - Nhận diện toàn bộ các tên miền CDN Video của Zalo (`video-stal-*.dlmd.me`, `.mdchat.me`).
  - Hiển thị thẻ video với ảnh thu nhỏ (thumbnail) chất lượng cao và nút Play tròn ở chính giữa thay vì bị hiển thị nhầm thành thẻ tập tin chứa mã băm token.

---

### 2. Trình phát Video In-App & Modal tương tác (`src/gui/modals.rs`)

* **Khắc phục lỗi xem video bị đứng ở frame đầu tiên:**
  - **Bản cũ:** Khi người dùng bấm vào video trong tin nhắn, modal mở ra nhưng chỉ hiển thị hình ảnh tĩnh của thumbnail (frame 1). Bấm nút phát hoặc phím cách không có phản hồi do lỗi đường dẫn ở mục 1.
  - **Bản mới:**
    - Hoàn thiện phương thức `render_video_player_modal`: Tự động tải video ngầm về thư mục tạm, theo dõi trạng thái tải theo thời gian thực (`ctx.request_repaint_after(250ms)`).
    - Ngay khi tải xong hoặc khi người dùng bấm phát, hệ thống tự động khởi chạy trình phát video mặc định (Windows Media Player, Movies & TV, v.v.) với đầy đủ âm thanh và hình ảnh mượt mà.
    - Thanh điều khiển dưới dạng Glassmorphism Pill sang trọng: Nút `▶ Phát video`, nhãn trạng thái `✅ Đã tải về bộ nhớ đệm`, nút `Mở trình phát máy` và nút `✕ Đóng (Esc)`.
    - Thanh công cụ phía trên: Hiển thị tiêu đề video, người gửi, thời gian, nút `📥 Lưu về máy` (lưu vào thư mục Download) và nút `📋 Sao chép liên kết`.

* **Tối ưu hóa các Modal và Dialog khác:**
  - Cải tiến tính thích ứng màn hình (Responsive Screen Fitting) cho hộp thoại Chuyển tiếp tin nhắn (Forward), Xác nhận đăng xuất, Xem trước liên kết (Link preview) và Danh bạ.

---

### 3. Thanh soạn thảo tin nhắn Composer (`src/gui/workspace.rs`)

* **Khắc phục triệt để lỗi "Dít" (Cramped / Compressed UI):**
  - **Bản cũ:** Ô nhập văn bản bị ép trong khung 34px chật hẹp, nút Gửi bị nén méo, khoảng cách giữa toolbar icon và ô nhập liệu chỉ có 4px tạo cảm giác dính chặt và thừa khoảng trống bất hợp lý bên dưới.
  - **Bản mới:**
    - Nâng chiều cao chuẩn của ô nhập liệu đơn dòng từ `34px` lên `38px`, font chữ 13.5pt với padding lọt lòng 9px trên dưới.
    - Tăng khoảng cách phân cách giữa thanh toolbar icon và ô nhập liệu từ `4px` lên `8px`.
    - Căn giữa tuyệt đối nút Gửi (Send) trong chiều cao ô nhập, viền bo góc 8px mềm mại.
    - Cơ chế mở rộng đa dòng co giãn đàn hồi (Elastic Multiline) từ 1 đến 5 dòng theo nội dung nhập thực tế mà không làm nhảy giao diện chat stream.
    - Tích hợp phát video nền và xử lý `BubbleAction::OpenVideoPlayer`.

---

### 4. Đồng bộ lịch sử tin nhắn Protobuf Sync V2 (`src/history_sync.rs`)

* **Bản cũ:** Bỏ qua Field 8 (`MetaData`) trong cấu trúc protobuf `proto.Sync2.Message.Message`. Chỉ đọc Field 7 (`content`) vốn chỉ chứa chuỗi token định danh trần của Zalo (`2aOboRCN...`). Thiếu bảng ánh xạ mã số loại tin nhắn `msg_type`.
* **Bản mới:**
  - Xây dựng bộ giải mã hoàn chỉnh cấu trúc Protobuf Sync V2:
    - Bổ sung cấu trúc `ParsedProtoAttach` và hàm parse Field 8 (`MetaData` -> repeated `Attach` struct).
    - Trích xuất chính xác các trường: `href`, `thumb`, `title`, `description`, `params`, `cat_id`, `id`.
  - Ánh xạ chính xác các mã loại tin nhắn Zalo PC:
    - `msg_type = 18` ➔ `chat.video.msg` (Video)
    - `msg_type = 19` ➔ `chat.file` (Tập tin)
    - `msg_type = 2` hoặc `11` ➔ `chat.photo` (Hình ảnh)
    - `msg_type = 1` ➔ `chat.sticker` (Sticker)
  - Tự động gán nhãn `[Video]` và `link_src = "Video Zalo"` thay vì lưu token thô làm nội dung tin nhắn.

---

### 5. Lưu trữ CSDL & Tự động phục hồi tin nhắn cũ (`src/store.rs`)

* **Bản cũ:** Khi nạp tin nhắn từ SQLite (`zalo_history.db`), các tin nhắn video cũ đã lưu bị mặc định là `MessageKind::Text` hoặc `MessageKind::File`, nội dung hiển thị là chuỗi băm `[Tập tin] 2aOboR...`.
* **Bản mới:**
  - Trong phương thức `hydrate_threads`: Bổ sung bộ lọc tự động phát hiện URL video (`is_video_url`).
  - Tự động chuyển đổi các bản ghi cũ sang `MessageKind::Video`, đặt `link_src = "Video Zalo"`, `link_title = "Video"` và chuẩn hóa lại nội dung từ `[Tập tin] 2aOboR...` thành `[Video]`.

---

### 6. Logic Chat, Nhóm & Xử lý URL (`src/chat.rs`)

* Bổ sung nhận diện các định dạng URL CDN mới của Zalo (`.dlmd.me`, `.mdchat.me`, `video-stal-*`).
* Hoàn thiện hàm giải quyết tên file đính kèm `resolve_attachment_file_name`.
* Cập nhật các endpoint rời nhóm và thao tác với nhóm theo tài liệu kỹ thuật đảo ngược (`zalo-re`).

---

### 7. Hệ thống Theme & Tài nguyên (`src/gui/theme.rs`, `src/assets/`)

* Bổ sung font chữ tiếng Việt `BeVietnamPro-SemiBold.ttf` đảm bảo hiển thị sắc nét trên mọi hệ điều hành.
* Hoàn thiện bảng màu Palette đạt chuẩn độ tương phản cao (High Contrast) cho cả Dark mode và Light mode.
* Bổ sung các icon Lucide vector đồng bộ cho toàn bộ giao diện (File text, Film, Mic, Paperclip, Check, Chevron, v.v.).

---

## 🧪 KẾT QUẢ KIỂM THỬ & XÁC MINH (VERIFICATION)

| Hạng mục kiểm tra | Lệnh thực hiện | Kết quả |
| :--- | :--- | :--- |
| **Kiểm tra cú pháp & kiểu dữ liệu** | `cargo check --manifest-path zlite/Cargo.toml --all-targets --locked` | **PASSED (0 lỗi)** |
| **Kiểm thử đơn vị tự động (Unit Tests)** | `cargo test --manifest-path zlite/Cargo.toml --locked --lib` | **PASSED (318/318 tests đạt)** |
| **Biên dịch Binary ứng dụng** | `cargo build --bin ctdozlite --manifest-path zlite/Cargo.toml --locked` | **SUCCESS (`ctdozlite.exe` sẵn sàng)** |

---

## 📝 HƯỚNG DẪN KHỞI CHẠY BẢN MỚI

Chạy lệnh sau tại thư mục dự án để khởi chạy trực tiếp ứng dụng:
```powershell
cargo run --locked
```
