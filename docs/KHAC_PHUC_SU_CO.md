# Xử lý sự cố thường gặp

Bảng tra cứu các mã lỗi trả về từ máy chủ Zalo và các vấn đề môi trường khi biên dịch hoặc vận hành QTzalo.

---

## 1. Mã lỗi giao thức Zalo

| Mã lỗi | Thông điệp từ server | Nguyên nhân | Cách xử lý trong ứng dụng |
|---|---|---|---|
| `18012` | `Invalid status` | Đã quét mã QR trên điện thoại nhưng chưa bấm xác nhận đăng nhập. | Tiếp tục long-polling chờ người dùng xác nhận trên điện thoại. |
| `18060` | `Invalid encryption protocol` | Payload gửi lên chưa được mã hóa qua `ParamCipher v2`. | Bọc tham số qua module `src/param_cipher.rs`. |
| `18022` | `Invalid Signature` | Sai chữ ký `signkey` do thừa hoặc thiếu tham số trong chuỗi băm MD5. | Chỉ đưa các tham số chính vào hàm tạo `signkey`. |
| `600` / `-102` | `zpw_sek` thiếu hoặc hết hạn | Cookie `zpw_sek` không được đính kèm cho domain đích hoặc phiên đã hết hạn trên server. | Tự động gắn `zpw_sek` cho các domain `*.zaloapp.com` / `*.chat.zalo.me`. Chỉ khi máy chủ trả mã này, ứng dụng mới yêu cầu quét lại QR; `session.json` chỉ được ghi đè sau khi đăng nhập mới thành công. |
| `114` | `Tham số không hợp lệ` | Sai kiểu dữ liệu của các trường JSON lồng nhau (ví dụ `threadIdLocalMsgId`). | Chuẩn hóa định dạng chuỗi JSON trước khi mã hóa `params`. |
| `429` | `Too Many Requests` | Gọi API tải danh bạ (`getfriends`) quá dày. | Chỉ gọi `getfriends` khi khởi tạo hoặc khi người dùng bấm làm mới; vòng lặp định kỳ chỉ gọi `get_last_msgs`. |

---

## 2. Lỗi biên dịch và hiển thị

### 2.1. Lỗi CMake khi biên dịch `audiopus_sys` / `opus-sys`
- **Triệu chứng:** CMake 4.0+ báo lỗi `Compatibility with CMake < 3.5 has been removed`.
- **Cách khắc phục:** Đặt biến môi trường `CMAKE_POLICY_VERSION_MINIMUM=3.5` trước khi chạy `cargo build`, hoặc cài sẵn `opus` và `pkg-config` qua trình quản lý gói hệ thống (`brew install opus pkg-config` trên macOS, `libopus-dev` trên Ubuntu/Debian).

### 2.2. Lỗi hiển thị tiếng Việt hoặc chữ Hán (ô vuông) trong GUI
- **Nguyên nhân:** Font mặc định của `egui` không chứa đủ ký tự tiếng Việt và hoàn toàn không có chữ Hán/Nhật/Hàn (CJK). Ứng dụng phải nạp font của hệ điều hành; trên một số bản Linux (ví dụ Fedora) font nằm ở đường dẫn khác hoặc chưa được cài.
- **Cách khắc phục:**
  - Windows: font `Segoe UI` và `Microsoft YaHei` có sẵn, không cần làm gì thêm.
  - Debian/Ubuntu: `sudo apt install fonts-noto-core fonts-noto-cjk`.
  - Fedora: `sudo dnf install google-noto-sans-fonts google-noto-sans-cjk-fonts`.
  - Arch: `sudo pacman -S noto-fonts noto-fonts-cjk`.
  - Sau khi cài font, khởi động lại ứng dụng.

### 2.3. Không giải mã được hình ảnh cuộc gọi video
- **Nguyên nhân:** Tính năng gọi video H.264 sử dụng `ffmpeg` để thu hình từ thiết bị camera và giải mã luồng video nhận về.
- **Cách khắc phục:** Kiểm tra `ffmpeg -version` trong terminal và đảm bảo `ffmpeg` đã nằm trong `PATH` của hệ thống.

---

## 3. Phiên đăng nhập, dữ liệu và giờ hiển thị

### 3.1. Mở lại app bị đăng xuất / phải quét QR lại
- **Bản cũ (ctdozlite ≤ 0.0.2):** mọi lỗi mạng hoặc HTTP khi kiểm tra phiên đều bị coi là "phiên hết hạn" và `session.json` bị xoá (upstream issue #21). Ngoài ra dữ liệu được ghi vào *thư mục đang đứng khi chạy app*, nên mở app từ chỗ khác sẽ không thấy phiên cũ.
- **QTzalo:** phiên được lưu cố định trong thư mục dữ liệu của người dùng và chỉ bị thay khi máy chủ Zalo xác nhận đã hết hạn (`600` / `-102`). Khi mất mạng, ứng dụng giữ nguyên phiên và thông báo không kết nối được.
- Nếu vẫn bị yêu cầu đăng nhập lại: kiểm tra trên điện thoại (*Cài đặt → Tài khoản và bảo mật → Thiết bị đăng nhập*) xem phiên máy tính có bị đăng xuất từ xa không.

### 3.2. Không thấy dữ liệu cũ sau khi nâng cấp
- Lần chạy đầu, QTzalo tự chuyển `session.json`, `zalo_history.db`, `threads_cache.json`, `app_settings.json` và `stickers_cache/` từ **thư mục làm việc hiện tại** vào thư mục dữ liệu, nếu ở đó chưa có dữ liệu.
- Nếu trước đây bạn chạy app từ một thư mục khác, hãy chạy QTzalo một lần từ chính thư mục đó, hoặc tự chép các file trên vào thư mục dữ liệu khi QTzalo đang tắt:
  - Windows: `%APPDATA%\QTzalo`
  - Linux: `~/.local/share/qtzalo`
  - macOS: `~/Library/Application Support/QTzalo`

### 3.3. Giờ tin nhắn lệch 7 tiếng
- **Nguyên nhân (bản cũ, upstream issue #4):** thời gian được định dạng theo UTC.
- **QTzalo:** mọi mốc giờ hiển thị theo múi giờ của hệ điều hành. Nếu vẫn lệch, kiểm tra cài đặt múi giờ của máy (Windows: *Settings → Time & language → Date & time*).

### 3.4. Danh sách bạn bè trống, lỗi `HTTP 404 ... tt-friend-wpa.zaloapp.com`
- **Nguyên nhân (upstream issue #22):** bảng domain từ máy chủ ghi đè domain đã kiểm chứng và được lưu vĩnh viễn vào `session.json`.
- **QTzalo:** domain đã kiểm chứng luôn được ưu tiên và được khôi phục mỗi khi nạp phiên. Nếu vẫn gặp lỗi, đăng xuất (`zalo-rust --logout`) rồi đăng nhập lại.

### 3.5. Windows SmartScreen chặn bộ cài
- Bộ cài chưa được ký số. Sau khi kiểm tra SHA256 với `SHA256SUMS.txt`, bấm **More info → Run anyway**. Xem [CAI_DAT.md](CAI_DAT.md).
