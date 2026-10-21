# Hướng Dẫn Khắc Phục Sự Cố (Troubleshooting Guide)

Tài liệu này tổng hợp các mã lỗi giao thức, sự cố giao diện và cách khắc phục khi vận hành **ctdozlite**.

---

## 1. Bảng tra cứu mã lỗi giao thức Zalo

| Mã lỗi | Thông điệp máy chủ | Nguyên nhân | Cách khắc phục đã tích hợp trong ctdozlite |
|:---:|---|---|---|
| **`18012`** | `Invalid status` | Điện thoại đã quét mã QR thành công, nhưng người dùng chưa bấm nút "Đăng nhập" trên app mobile. | Tiếp tục vòng lặp polling 800ms chờ người dùng bấm xác nhận (không coi là lỗi). |
| **`18060`** | `Invalid encryption protocol` | Tham số gửi lên server ở dạng JSON trần mà không được bọc qua ParamCipher v2. | Sử dụng mô đun `src/param_cipher.rs` để mã hóa payload trước khi gửi. |
| **`18022`** | `Invalid Signature` | Chữ ký `signkey` bị sai do tham số phụ (`ref`, `nretry`) vô tình bị đưa vào hash MD5. | Chỉ tính `signkey` trên các tham số chính thức; tham số phụ được nối sau khi ký. |
| **`600`** | `zpw_sek bị thiếu hoặc không đúng` | Cookie phiên xác thực `zpw_sek` chưa được truyền trong header `Cookie` hoặc phiên đã hết hạn. | Hệ thống tự động bắt cookie từ `Set-Cookie`, tự động nạp cookie domain `.zalo.me` và tự động chuyển về màn hình quét QR mới nếu phiên hết hạn. |
| **`102`** | `The session key was improperly submitted...` | Khóa phiên làm việc trên server đã bị thu hồi hoặc timeout sau thời gian dài không hoạt động. | Kích hoạt cơ chế `s.validate_session()`, tự động dọn dẹp file phiên cũ và sinh QR mới. |
| **`114`** | `Tham số không hợp lệ` | Tham số `threadIdLocalMsgId` truyền dạng Object thay vì JSON stringify dictionary (`"{}"`). | Đã chuẩn hóa chuỗi JSON stringify map trong `src/chat.rs`. |
| **`429`** | `429 Too Many Requests (Nginx)` | Gửi request tải danh bạ bạn bè (`getfriends`) với tần suất quá cao làm kích hoạt WAF. | Tách rời polling: Vòng lặp 3s chỉ gọi `get_last_msgs` siêu nhẹ, không gọi `getfriends` định kỳ. |
| **`404`** | `404 Not Found` | Gọi nhầm domain cũ của PC bundle (ví dụ: `tt-friend-wpa`). | Đã trỏ thẳng sang `tt-profile-wpa.zaloapp.com` theo đúng phiên làm việc thực tế. |

---

## 2. Sự cố giao diện và hiển thị

### 2.1. Lỗi hiển thị chữ tiếng Việt thành các ô vuông `□` (Tofu glyphs) trong GUI
- **Nguyên nhân:** Thư viện `egui` mặc định chỉ kèm font Latin cơ bản (ProggyClean), không có các ký tự có dấu tiếng Việt (`ế, ủ, ữ, ắ, ạ, đ, ộ, ệ, ử...`).
- **Cách xử lý:** Ứng dụng tự động nạp font hệ thống Windows **Segoe UI** (`C:\Windows\Fonts\segoeui.ttf`) hoặc Arial/Tahoma khi khởi chạy (`setup_custom_fonts`), đảm bảo 100% tiếng Việt hiển thị sắc nét.

### 2.2. Khung viền TUI bị méo mó, xô lệch trên PowerShell
- **Nguyên nhân:** Các biểu tượng Emoji Unicode (👤, 👥, 📱) có độ rộng 2 ô (double-width), làm terminal tính toán sai chiều ngang và làm lệch đường viền dọc `│`.
- **Cách xử lý:** Đã chuyển toàn bộ sang các tag đơn chiều rộng chuẩn (`[User]`, `[Group]`, `[Cloud]`) và khung viền phẳng ANSI `BorderType::Plain` (`┌─┐`, `│ │`, `└─┘`).

### 2.3. Camera điện thoại khó quét mã QR
- **Nguyên nhân:** Khi bật Dark Mode, nền đen xung quanh ăn sát vào 3 ô vuông định vị của mã QR khiến camera không nhận diện được viền.
- **Cách xử lý:** Mã QR trong ctdozlite được bọc trong khung thẻ trắng tinh bo góc (`Color32::WHITE`) với Quiet Zone rộng 4 ô chuẩn quốc tế và kích thước phóng to `280x280 px`.

### 2.4. Không nhận được tin nhắn từ nick lạ
- **Nguyên nhân:** Danh sách yêu cầu chỉ hỏi tin nhắn của các ID đã quen; tin nhắn đến từ người lạ mang `idTo = "0"` và người gửi là `uidFrom`.
- **Cách xử lý:** Đã cập nhật gửi `{}` để nhận toàn bộ tin nhắn mới của tất cả mọi người, bổ sung hàm `extract_message_thread_id` trích xuất đúng ID người gửi và tự động tra cứu tên hiển thị thật qua `getminiprofiles`.
