# Xử lý sự cố thường gặp

Bảng tra cứu các mã lỗi trả về từ máy chủ Zalo và các vấn đề môi trường khi biên dịch hoặc vận hành `ctdozlite`.

---

## 1. Mã lỗi giao thức Zalo

| Mã lỗi | Thông điệp từ server | Nguyên nhân | Cách xử lý trong ứng dụng |
|---|---|---|---|
| `18012` | `Invalid status` | Đã quét mã QR trên điện thoại nhưng chưa bấm xác nhận đăng nhập. | Tiếp tục long-polling chờ người dùng xác nhận trên điện thoại. |
| `18060` | `Invalid encryption protocol` | Payload gửi lên chưa được mã hóa qua `ParamCipher v2`. | Bọc tham số qua module `src/param_cipher.rs`. |
| `18022` | `Invalid Signature` | Sai chữ ký `signkey` do thừa hoặc thiếu tham số trong chuỗi băm MD5. | Chỉ đưa các tham số chính vào hàm tạo `signkey`. |
| `600` / `-102` | `zpw_sek` thiếu hoặc hết hạn | Cookie `zpw_sek` không được đính kèm cho domain đích hoặc phiên đã hết hạn trên server. | Tự động gắn `zpw_sek` cho toàn bộ các domain `*.zaloapp.com` / `*.chat.zalo.me`; nếu hết hạn thì xóa `session.json` và tạo mã QR mới. |
| `114` | `Tham số không hợp lệ` | Sai kiểu dữ liệu của các trường JSON lồng nhau (ví dụ `threadIdLocalMsgId`). | Chuẩn hóa định dạng chuỗi JSON trước khi mã hóa `params`. |
| `429` | `Too Many Requests` | Gọi API tải danh bạ (`getfriends`) quá dày. | Chỉ gọi `getfriends` khi khởi tạo hoặc khi người dùng bấm làm mới; vòng lặp định kỳ chỉ gọi `get_last_msgs`. |

---

## 2. Lỗi biên dịch và hiển thị

### 2.1. Lỗi CMake khi biên dịch `audiopus_sys` / `opus-sys`
- **Triệu chứng:** CMake 4.0+ báo lỗi `Compatibility with CMake < 3.5 has been removed`.
- **Cách khắc phục:** Đặt biến môi trường `CMAKE_POLICY_VERSION_MINIMUM=3.5` trước khi chạy `cargo build`, hoặc cài sẵn `opus` và `pkg-config` qua trình quản lý gói hệ thống (`brew install opus pkg-config` trên macOS, `libopus-dev` trên Ubuntu/Debian).

### 2.2. Lỗi hiển thị tiếng Việt trong GUI
- **Nguyên nhân:** Font mặc định của `egui` không chứa đầy đủ bảng mã Unicode tiếng Việt.
- **Cách khắc phục:** Hàm `setup_custom_fonts` trong `src/gui/theme.rs` tự động tìm và nạp các font hệ thống hỗ trợ tiếng Việt (`Segoe UI`, `Arial`, `Tahoma`, `NotoSans`, `DejaVuSans`, `Ubuntu`). Nếu chạy trên bản Linux tối giản chưa có font, hãy cài gói `fonts-noto-core` hoặc `fonts-dejavu-core`.

### 2.3. Không giải mã được hình ảnh cuộc gọi video
- **Nguyên nhân:** Tính năng gọi video H.264 sử dụng `ffmpeg` để thu hình từ thiết bị camera và giải mã luồng video nhận về.
- **Cách khắc phục:** Kiểm tra `ffmpeg -version` trong terminal và đảm bảo `ffmpeg` đã nằm trong `PATH` của hệ thống.

---

## 3. Sự cố phiên đăng nhập

### 3.1. "Phiên đăng nhập trong session.json đã hết hạn" dù vừa đăng nhập

**Đây là lỗi đã biết**, không phải phiên của bạn thật sự hết hạn: [#21](https://github.com/ctdoteam1110/ctdozlite/issues/21).

Nguyên nhân: ứng dụng coi **mọi lỗi mạng** (mất Wi-Fi, DNS hỏng, HTTP 404/500) là "phiên hết hạn", rồi xoá luôn `session.json`. Vì GUI gọi kiểm tra phiên ngay khi khởi động, chỉ cần mở app lúc mạng chập là bị đăng xuất.

**Cách giảm thiểu tạm thời:**

1. Giữ một bản sao `session.json` ở chỗ khác sau khi đăng nhập thành công.
2. Nếu bị đăng xuất oan, đóng app, chép bản sao trở lại, rồi mở lại khi mạng đã ổn định.
3. Nếu vẫn bị, chạy `zalo-rust --gui --dev` để xem lỗi thật trong Dev Inspector (thường là `HTTP 404` hoặc lỗi DNS, không phải `600`).

Đăng xuất bằng `--logout` hiện **xoá luôn lịch sử cục bộ** `zalo_history.db`. Sao lưu file này trước khi đăng xuất nếu muốn giữ lịch sử.

### 3.2. TUI trống, báo `HTTP 404 Not Found from https://tt-friend-wpa.zaloapp.com/...`

**Đây là lỗi đã biết**: [#22](https://github.com/ctdoteam1110/ctdozlite/issues/22).

Nguyên nhân: địa chỉ API bạn bè bị ghi đè bằng tên miền cũ `.zaloapp.com`; tên miền đúng là `.chat.zalo.me`. Địa chỉ sai được lưu vào `session.json` nên lần mở sau vẫn lỗi.

**Cách sửa tạm thời:** mở `session.json` trong thư mục làm việc, tìm khối `"domains"` và sửa hai dòng sau thành:

```json
"friend": "https://tt-friend-wpa.chat.zalo.me",
"profile": "https://tt-profile-wpa.chat.zalo.me"
```

Lưu file rồi mở lại ứng dụng. Lưu ý: ứng dụng có thể ghi đè lại khi đăng nhập mới. Đăng nhập lại từ đầu (xoá `session.json`) cũng là cách tạm thời khác.

### 3.3. Chạy nhiều tài khoản bị đăng xuất lẫn nhau

Ứng dụng lưu `session.json`, `zalo_history.db` và `app_settings.json` **theo thư mục hiện tại**. Hai cửa sổ chạy trong cùng thư mục sẽ ghi đè phiên của nhau.

**Cách làm đúng:** mỗi tài khoản một thư mục riêng.

```bash
mkdir -p ~/zalo-work && cd ~/zalo-work && /đường/dẫn/zalo-rust --gui
```

Không chạy từ thư mục không có quyền ghi (ví dụ `C:\Program Files`) — phiên sẽ không được lưu.

### 3.4. Đóng app xong mở lại phải quét QR

Kiểm tra ba điểm:

1. Thư mục làm việc có quyền ghi không (mục 3.3).
2. Có đang chạy từ thư mục khác so với lần đăng nhập không.
3. Có dính lỗi [#21](https://github.com/ctdoteam1110/ctdozlite/issues/21) không — xem mục 3.1.
