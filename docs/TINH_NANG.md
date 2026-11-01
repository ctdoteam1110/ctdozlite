# Tính năng & giới hạn

Trang này cho biết **cái gì đang chạy, cái gì chạy một phần, cái gì chưa có** — để bạn không mất thời gian báo lại những lỗi đã biết.

> **Kiểm chứng tại commit `73849bd`** (nhánh `main`). Bảng này được cập nhật thủ công mỗi lần release.
> Nếu bạn đang dùng bản build cũ hơn, một số dòng có thể đã khác.

## Cách đọc

| Ký hiệu | Nghĩa |
|---|---|
| ✅ | Hoạt động bình thường |
| 🟡 | Hoạt động một phần — có giới hạn được ghi rõ |
| 🐞 | Đã biết lỗi — đang có issue theo dõi |
| ⛔ | Chưa hỗ trợ |
| 🚫 | Không nằm trong kế hoạch |

Cột **Issue** trỏ tới nơi đang theo dõi. Nếu issue đã đóng, tính năng đã được sửa.

---

## Đăng nhập & phiên làm việc

| Tính năng | Trạng thái | Ghi chú | Issue |
|---|---|---|---|
| Đăng nhập bằng mã QR | ✅ | Quét bằng app Zalo trên điện thoại | |
| Đăng nhập bằng số điện thoại + mật khẩu | ✅ | `--phone`, `--pass` | |
| Tự khôi phục phiên từ `session.json` | 🐞 | Lỗi mạng hoặc HTTP 404 bị coi là "hết hạn" → xoá phiên, bắt quét QR lại | [#21](https://github.com/ctdoteam1110/ctdozlite/issues/21) |
| Đăng xuất | 🟡 | Hiện xoá luôn lịch sử cục bộ `zalo_history.db` | [#21](https://github.com/ctdoteam1110/ctdozlite/issues/21) |
| Nhiều tài khoản cùng lúc | 🟡 | Mỗi cửa sổ phải chạy trong **một thư mục làm việc riêng**. Chưa có `--profile` | |

> ⚠️ **Quan trọng với đa tài khoản:** dữ liệu (`session.json`, `zalo_history.db`, `app_settings.json`) đang lưu **theo thư mục hiện tại**. Muốn chạy 2 tài khoản, mở terminal ở 2 thư mục khác nhau. Chạy cùng thư mục sẽ ghi đè phiên của nhau. Chạy từ thư mục không có quyền ghi (ví dụ `Program Files`) sẽ không lưu được phiên.

---

## Nhắn tin

| Tính năng | Trạng thái | Ghi chú | Issue |
|---|---|---|---|
| Gửi/nhận tin nhắn văn bản (1-1 và nhóm) | ✅ | | |
| Trả lời trích dẫn | ✅ | Chuột phải vào bong bóng tin nhắn | |
| Chuyển tiếp tin nhắn | ✅ | | |
| Thả cảm xúc (reaction) | ✅ | | |
| Thu hồi tin nhắn | ✅ | Thu hồi với mọi người | |
| Xoá tin nhắn phía tôi | ✅ | Không ảnh hưởng người khác | |
| Ghim / đánh dấu sao | 🟡 | Tin nhắn ghim hiển thị sai thứ tự trong danh sách | [#9](https://github.com/ctdoteam1110/ctdozlite/issues/9) |
| Gửi ảnh và tệp đính kèm | ✅ | | |
| Gửi sticker, gợi ý sticker khi gõ | ✅ | Có kho sticker và tìm theo từ khóa tiếng Việt | |
| Dán ảnh trực tiếp (`Ctrl+V`) | 🟡 | Đã kiểm chứng trên Windows; nền tảng khác dùng nút đính kèm | |
| Xem trước liên kết (OpenGraph) | ✅ | | |
| Tin nhắn thoại | ⛔ | Chưa ghi âm và gửi được tin nhắn thoại | |
| Chỉnh sửa tin nhắn đã gửi | ⛔ | | |

---

## Hội thoại & lịch sử

| Tính năng | Trạng thái | Ghi chú | Issue |
|---|---|---|---|
| Danh sách hội thoại (bạn bè, nhóm, Cloud của tôi) | 🐞 | Có thể trả `HTTP 404` từ `tt-friend-wpa.zaloapp.com` | [#22](https://github.com/ctdoteam1110/ctdozlite/issues/22) |
| Tên nhóm và ảnh đại diện nhóm | 🐞 | Một số nhóm hiện tên chung chung | [#8](https://github.com/ctdoteam1110/ctdozlite/issues/8) |
| Nhận tin nhắn mới theo thời gian thực | ✅ | Polling + WebSocket | |
| Tìm kiếm hội thoại (không dấu) | ✅ | Ô tìm kiếm phía trên danh sách bên trái | |
| Tìm kiếm trong nội dung tin nhắn | ⛔ | Chỉ tìm được tên hội thoại | |
| Lưu lịch sử cục bộ (SQLite) | ✅ | `zalo_history.db` trong thư mục làm việc | |
| Tải lịch sử cũ từ cloud Zalo | 🟡 | Chỉ khoảng **50 tin nhắn gần nhất** mỗi hội thoại. Chưa tải toàn bộ | [#19](https://github.com/ctdoteam1110/ctdozlite/issues/19) |
| Nút "Đồng bộ tin nhắn" | 🐞 | Thực chất chỉ **làm mới** danh sách hội thoại, không tải lịch sử. Nhãn gây hiểu nhầm | [#23](https://github.com/ctdoteam1110/ctdozlite/issues/23) |
| Di chuyển dữ liệu từ máy khác | ⛔ | Chưa có công cụ migrate | [#9](https://github.com/ctdoteam1110/ctdozlite/issues/9) |

---

## Bạn bè

| Tính năng | Trạng thái | Ghi chú | Issue |
|---|---|---|---|
| Tìm người dùng bằng số điện thoại | ✅ | | |
| Gửi lời mời kết bạn | ✅ | | |
| Chấp nhận / từ chối lời mời | ✅ | | |
| Huỷ lời mời đã gửi | ✅ | | |

---

## Gọi thoại & video

| Tính năng | Trạng thái | Ghi chú | Issue |
|---|---|---|---|
| Gọi thoại 1-1 | 🟡 | Đã có luồng gọi; chất lượng phụ thuộc mạng | |
| Gọi video | 🟡 | Cần `ffmpeg` trong `PATH` để thu và giải mã hình | |
| Gọi nhóm | 🟡 | | |

---

## Giao diện & nền tảng

| Tính năng | Trạng thái | Ghi chú | Issue |
|---|---|---|---|
| Giao diện đồ họa (GUI) | ✅ | `--gui`, mặc định | |
| Giao diện terminal (TUI) | 🟡 | Danh sách bạn bè có thể trống nếu dính lỗi domain | [#22](https://github.com/ctdoteam1110/ctdozlite/issues/22) |
| Chủ đề sáng / tối | ✅ | | |
| Thông báo tin nhắn mới | 🟡 | Đếm số chưa đọc trên tiêu đề cửa sổ, nhấp nháy taskbar và âm thanh; chưa có toast của hệ điều hành | |
| Hiển thị tiếng Việt và tiếng Trung | 🟡 | Phụ thuộc font hệ thống; xem [Xử lý sự cố](KHAC_PHUC_SU_CO.md) | [#6](https://github.com/ctdoteam1110/ctdozlite/issues/6) |
| Tự động cập nhật | ⛔ | Phải tải bản mới thủ công từ Releases | [#12](https://github.com/ctdoteam1110/ctdozlite/issues/12) |
| Plugin / Theme của bên thứ ba | ⛔ | Chưa có API mở rộng | [#7](https://github.com/ctdoteam1110/ctdozlite/issues/7) |
| iOS / Android | 🚫 | Chỉ có desktop | |

---

## Chưa có, và đã có issue theo dõi

- Tải toàn bộ lịch sử trò chuyện từ cloud → [#19](https://github.com/ctdoteam1110/ctdozlite/issues/19)
- Sửa lỗi phiên bị coi là hết hạn → [#21](https://github.com/ctdoteam1110/ctdozlite/issues/21)
- Sửa lỗi 404 danh sách bạn bè → [#22](https://github.com/ctdoteam1110/ctdozlite/issues/22)
- Làm rõ nút "Đồng bộ tin nhắn" → [#23](https://github.com/ctdoteam1110/ctdozlite/issues/23)

Xem toàn bộ issue đang mở: [github.com/ctdoteam1110/ctdozlite/issues](https://github.com/ctdoteam1110/ctdozlite/issues)

---

## Báo một lỗi mới

Trước khi mở issue, kiểm tra [trang Đã biết](DA_BIET.md). Nếu chưa có, mở issue kèm:

1. Hệ điều hành và phiên bản (Windows 11, macOS 15, Fedora 41…).
2. Phiên bản app (góc trên bên trái, `Native v…`).
3. Chế độ đang chạy: GUI hay TUI.
4. Bước tái hiện, càng ngắn càng tốt.
5. Log: chạy lại với `--dev` và dán phần lỗi.

Xem [Hướng dẫn sử dụng](HUONG_DAN_SU_DUNG.md) để biết chi tiết các tham số.
