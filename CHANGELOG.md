# Changelog

Mọi thay đổi đáng chú ý của `ctdozlite` được ghi nhận tại tệp này theo chuẩn [Keep a Changelog](https://keepachangelog.com/vi/1.1.0/) và [Semantic Versioning](https://semver.org/).
Xem thêm quy ước tại [ADR 0003 — Chính sách Changelog & Release Notes](docs/adr/0003-changelog-policy.md).

## [Unreleased]

### Fixed (Sửa lỗi)
- **Linux: dữ liệu không còn phụ thuộc thư mục khởi chạy (`src/data_dir.rs`):** Bản Linux lưu phiên, lịch sử tin nhắn và cache vào `$XDG_DATA_HOME/ctdozlite` (mặc định `~/.local/share/ctdozlite`) thay vì thư mục làm việc, nên mở từ menu hay terminal đều thấy cùng dữ liệu. Lần đầu chạy, dữ liệu cũ ở thư mục làm việc hoặc `~` được tự sao chép sang (bản gốc giữ nguyên).

## [0.1.2] - 2026-11-16

### Added (Tính năng mới & Cập nhật)
- **Đồng bộ tin nhắn Sync V2 di động tối ưu (`src/chat.rs`, `src/gui/modals.rs`):** Truyền `tempKey` quét mã QR vào gói `cmd 590` và kích hoạt song song phiên HTTP `cmd 12888`; thanh tiến trình đồng bộ hiển thị trực quan phần trăm `%`, trạng thái giải mã gói tin từ điện thoại và văn bản hướng dẫn rõ ràng.
- **Thẻ cuộc gọi chuyên nghiệp (Call Cards) (`src/history.rs`, `src/gui/messages.rs`):** Chuẩn hóa toàn bộ tin nhắn cuộc gọi thoại/video thành thẻ cuộc gọi có icon trực quan, phân biệt chiều cuộc gọi (đến, đi, nhỡ, đã hủy), thời lượng hiển thị chi tiết (ví dụ: `0 phút 20 giây`) và nút "Gọi lại".
- **Căn lề Album ảnh chuẩn xác (`src/gui/messages.rs`):** Khắc phục triệt để lỗi bong bóng lưới album ảnh gửi đi bị lệch giữa, tự động tính toán chính xác bề rộng hiển thị (`album_rendered_width`) và bám sát mép phải (flush right) đồng bộ với giao diện chung.
- **Gửi nhiều tệp và album ảnh dạng lưới ([PR #127](https://github.com/ctdoteam1110/ctdozlite/pull/127) bởi @gr219):** Hỗ trợ đính kèm nhiều tệp/thư mục, gom ảnh gửi đi thành album `groupLayoutId` và hiển thị lưới 2–3 cột gọn gàng trong bong bóng chat.
- **Tải Sync V2 WASM trực tiếp từ Zalo PC app.asar ([PR #145](https://github.com/ctdoteam1110/ctdozlite/pull/145) bởi @gr219):** Tự động phát hiện và trích xuất `libzproto_wasm` từ Zalo PC cài sẵn trên máy mà không cần tải thêm bên ngoài.
- **Console TUI Workspace & LLM Agent Loop ([PR #137](https://github.com/ctdoteam1110/ctdozlite/pull/137), [PR #144](https://github.com/ctdoteam1110/ctdozlite/pull/144) bởi @hoanglong149):** Mở rộng giao diện dòng lệnh TUI, tích hợp nhà cung cấp LLM, vòng lặp Agent tự động, khóa luồng MCP và cổng bảo mật riêng tư.
- **Cầu nối 2 chiều Telegram ↔ Zalo ([PR #143](https://github.com/ctdoteam1110/ctdozlite/pull/143) bởi @hoanglong149):** Hỗ trợ relay tin nhắn văn bản giữa Telegram bot và Zalo hội thoại kèm bộ đệm hạn ngạch LRU và cơ chế chống dội tin (echo-guard).
- **Trình chuyển đổi đa tài khoản nhanh (Multi-Account Switcher) ([PR #142](https://github.com/ctdoteam1110/ctdozlite/pull/142) bởi @ducseul):** Cho phép xem danh sách, chuyển đổi tức thì và quản lý phiên của nhiều tài khoản Zalo lưu trên máy.
- **Thiết kế lại Card tệp đính kèm & Tối ưu phát video ([PR #148](https://github.com/ctdoteam1110/ctdozlite/pull/148) bởi @Fin12n):** Giao diện thẻ tệp hiện đại, xử lý mượt mà luồng phát video và tinh chỉnh trải nghiệm xem trước tệp.
- **Xem trước tài liệu trong ứng dụng (`src/doc_preview.rs`):** Hỗ trợ đọc nhanh nội dung docx, xlsx, pptx, pdf, txt, csv, md ngay trong bong bóng chat.
- **Thư mục dữ liệu chuẩn macOS & Hoàn thiện CI/CD Cross-Platform (`tests/data_directory_cli.rs`, `.github/workflows/ci-release.yml`):** Tuân thủ đường dẫn dữ liệu chuẩn `Application Support/ctdozlite` trên macOS, đảm bảo toàn bộ pipeline build và kiểm thử đa nền tảng (Linux, Windows, macOS) chạy hoàn toàn 100% xanh.

### Fixed (Sửa lỗi & Tối ưu hiệu năng)
- **Sửa lỗi hiển thị chuỗi thời lượng số trần trên tin nhắn cuộc gọi cũ:** Nâng cấp migration cơ sở dữ liệu SQLite cục bộ để chuẩn hóa các tin nhắn lưu dạng chuỗi thô (`0:18`, `0:28`, ...) về định dạng tin nhắn cuộc gọi chuẩn kèm metadata.
- **Giảm tải CPU 100% khi chạy nền ([PR #136](https://github.com/ctdoteam1110/ctdozlite/pull/136) bởi @hoanglong149):** Khống chế chu kỳ vẽ lại poll-driven layout, lưu bộ đệm lọc/sắp xếp danh sách sidebar giúp ứng dụng hoạt động êm ái, tiết kiệm pin.
- **Bảo toàn URL khi chuyển tiếp liên kết Rich Link ([PR #134](https://github.com/ctdoteam1110/ctdozlite/pull/134) bởi @cuongdm75):** Đảm bảo giữ nguyên fallback văn bản chứa liên kết gốc khi chuyển tiếp tin nhắn preview sang người nhận khác.
- **Cải tiến dán nhiều tệp từ clipboard:** Dán cộng dồn tệp vào hàng chờ thay vì ghi đè, hỗ trợ trọn vẹn danh sách tệp từ File Explorer, macOS Finder và Linux file manager.

### Vinh danh đóng góp từ Cộng đồng & Tác giả (Authors & Contributors)
- **Đồng tác giả (Authors):** TIPC1110 (`tienichpc1110@gmail.com`) & Đỗ Thành ([@thanhdo1110](https://github.com/thanhdo1110)).
- **Cộng đồng đóng góp PRs xuất sắc cho v0.1.2:**
  - **@hoanglong149:** [PR #136](https://github.com/ctdoteam1110/ctdozlite/pull/136) (tối ưu CPU idle), [PR #137](https://github.com/ctdoteam1110/ctdozlite/pull/137) & [PR #144](https://github.com/ctdoteam1110/ctdozlite/pull/144) (Console TUI + LLM Agent Workspace), [PR #143](https://github.com/ctdoteam1110/ctdozlite/pull/143) (Telegram ↔ Zalo bridge).
  - **@gr219:** [PR #127](https://github.com/ctdoteam1110/ctdozlite/pull/127) (Đính kèm nhiều tệp & Album ảnh dạng lưới), [PR #145](https://github.com/ctdoteam1110/ctdozlite/pull/145) (Trích xuất Sync v2 WASM từ Zalo app.asar).
  - **@Fin12n:** [PR #148](https://github.com/ctdoteam1110/ctdozlite/pull/148) (Redesign file card UI, video playback & sync polish).
  - **@ducseul:** [PR #142](https://github.com/ctdoteam1110/ctdozlite/pull/142) (Multi-account switcher UI & account management).
  - **@cuongdm75:** [PR #134](https://github.com/ctdoteam1110/ctdozlite/pull/134) (Preserve URL on rich link forward fallback).
  - **@nguyenhoclaptrinh, @ziq3, @Leoaxo098:** Nghiên cứu và đề xuất vá lỗi Sync V2 mobile handshake, Windows console flash và reaction banner replay.

## [0.1.0] - 2026-11-09

### Added (Tính năng mới & Cập nhật)
- **Đồng bộ tin nhắn Sync v2 di động bằng Wasmtime (`src/zproto_host.rs`, `src/history_sync.rs`):** Nhúng trực tiếp engine WASM `libzproto_wasm` của Zalo PC thông qua runtime Wasmtime thuần Rust, loại bỏ hoàn toàn việc phụ thuộc vào Node.js bên ngoài khi thực hiện đồng bộ tin nhắn từ thiết bị di động (hỗ trợ bắt tay Curve25519/X25519, giải mã session key và streaming gói tin).
- **Giao thức ghim và bỏ ghim tin nhắn qua API máy chủ (`src/chat.rs`, `src/endpoints.rs`):** Tích hợp reverse-engineering các endpoint bảng tin Zalo (`friendboard-wpa` cho chat 1-1 và `groupboard-wpa` cho nhóm), cấu trúc hóa dữ liệu `MSG_TOPIC` với đầy đủ định danh tin nhắn (`global_msg_id`, `client_msg_id`).
- **Ghi nhận đóng góp từ cộng đồng:** Bổ sung các đóng góp tiêu biểu từ @gr219 (xử lý `file_done`, spawn helper ETXTBSY), @Leoaxo098 (khóa composer nhóm bị giới hạn), và @hoanglong149 (nghiên cứu WASM Sync v2) vào danh sách vinh danh và cập nhật quy mô codebase lên hơn 73.300+ dòng.

### Fixed (Sửa lỗi & Tinh chỉnh giao diện)
- **Sửa lỗi gửi lại thông báo và đếm lại tin nhắn chưa đọc khi mở lại ứng dụng ([#122](https://github.com/ctdoteam1110/ctdozlite/issues/122)):** Khi khởi động ứng dụng, nạp trực tiếp điểm đọc từ đúng cơ sở dữ liệu tài khoản (`accounts/<uin>_history.db`) thay vì file mặc định. Bổ sung bộ lọc so sánh `last_seen_id` khi đồng bộ nền để không bao giờ đếm lại các tin nhắn đã đọc thành tin chưa đọc.
- **Tối ưu hóa layout Responsive & ngăn vỡ khung chat trên màn hình nhỏ ([#123](https://github.com/ctdoteam1110/ctdozlite/issues/123)):** Khôi phục các điểm ngắt responsive (`is_compact_w`, `is_narrow_w`), tự động ẩn thanh thông tin drawer bên phải khi cửa sổ quá hẹp. Cho phép co giãn sidebar hội thoại linh hoạt theo tỷ lệ (tối đa 42% chiều rộng màn hình). Thêm cơ chế `.truncate()` ở tiêu đề chat để ngăn tràn chữ đè lên cụm nút gọi điện/video.
- **Ẩn các liên hệ chưa có tin nhắn trong danh sách chat chính:** Chỉ hiển thị những người dùng đã có phát sinh tin nhắn qua lại trong danh sách hội thoại bên trái (chuẩn hành vi Zalo gốc), ẩn các tài khoản danh bạ trống giúp sidebar gọn gàng hơn.
- **Sửa lỗi ký tự ô vuông (Tofu) và thiết kế lại popup thông báo:** Thay thế toàn bộ ký tự Unicode checkmark `✓` và dấu đóng `✕` không được font hỗ trợ bằng icon vector Lucide (`lucide::CHECK`, `lucide::X`). Mở rộng kích thước, padding và chiều cao thanh nhập liệu trong popup thông báo nổi góc màn hình.
- **Hợp nhất cảm xúc tin nhắn (Reaction):** Gộp các phản ứng cùng loại emoji thành một biểu tượng duy nhất kèm số lượng tổng, bổ sung hộp thoại chi tiết xem danh sách người thả cảm xúc.

## [0.0.9] - 2026-11-08

### Added (Tính năng mới & Cập nhật)
- **Họp nhóm Zavi trực tiếp (`src/call.rs`, `src/gui/workspace.rs`):** Tích hợp nút gọi nhóm "Họp Zavi" trên header thanh công cụ nhóm, hỗ trợ báo hiệu và kết nối luồng hội nghị đa điểm Zavi.
- **Popup thông báo nổi góc màn hình (Corner Notification) & Âm thanh báo (`src/gui/modals.rs`, `src/media.rs`):** Hiển thị thẻ thông báo tin nhắn mới ngoài desktop kèm nút mở chat nhanh, trả lời trực tiếp từ thông báo và phát âm thanh chuông báo tin nhắn (`play_notification_chime`).
- **Nút cuộn nhanh xuống tin nhắn mới nhất (`src/gui/workspace.rs`):** Tự động hiển thị nút mũi tên trôi ở góc dưới bên phải khung chat khi cuộn lên xem tin nhắn cũ, bấm để nhảy tức thì về tin nhắn mới nhất và triệt tiêu hiện tượng giật hình (flicker).
- **Hỗ trợ ghim tin nhắn cá nhân và đánh dấu sao cục bộ:** Bổ sung trường `is_pinned`, `is_starred` trong bong bóng tin nhắn và thanh thông báo tin ghim ở đầu cuộc trò chuyện.

### Fixed (Sửa lỗi)
- **Sửa lỗi đơ cuộc gọi video (Video call freeze):** Tinh chỉnh tiến trình bắt tay ZRTP và bộ đệm RTP/RTCP của luồng webcam/video, tự động đóng màn hình gọi khi đối phương đã nghe máy ở thiết bị khác.
- **Bảo mật & Tránh lộ tên file tạm:** Sửa lỗi đặt tên ảnh dán từ clipboard (`clip_preview_*`), ngăn ngừa rủi ro SSRF và loại trừ tệp lưu khóa riêng tư `sync_v2_keys.json` khỏi kho Git.
- **Tập tin đính kèm bất đồng bộ ([#85](https://github.com/ctdoteam1110/ctdozlite/issues/85)):** Đợi tín hiệu `file_done` trước khi hoàn tất gửi tệp đính kèm lớn, khắc phục lỗi HTTP 405 trên nút kẹp giấy.

## [0.0.8] - 2026-11-04

### Added (Tính năng mới & Cập nhật)
- **Hoàn thiện đồng bộ dữ liệu di động Android (`src/history_sync.rs`):** Hỗ trợ giải nén và giải mã toàn diện các tệp container `.db.crypt` (RSA 2048 + AES-256-CBC + LZMA2) từ bản sao lưu Zalo Android.
- **Nghiên cứu & Bắt tay Sync v2 iOS (Curve25519 / X25519 Double-Ratchet):** Triển khai giao thức bắt tay trao đổi khóa phiên cho việc đồng bộ từ thiết bị Apple iOS.
- **Quản lý đa tài khoản (Multi-Profile) (`src/account_manager.rs`):** Cho phép lưu trữ và chuyển đổi nhanh giữa nhiều tài khoản Zalo trên cùng một máy, lưu cơ sở dữ liệu tách biệt theo định danh (`accounts/<uin>_history.db`).
- **Xem Nhật ký mạng xã hội Zalo (`src/feed.rs`, `src/bin/view_feed.rs`):** Hỗ trợ nạp và đọc bảng tin nhật ký bạn bè từ cache SQLite cục bộ.

### Fixed (Sửa lỗi)
- **Khóa ô nhập liệu khi bị chặn chat trong nhóm ([#97](https://github.com/ctdoteam1110/ctdozlite/issues/97)):** Nhận diện cờ `posting_restricted` từ cấu hình nhóm Zalo và vô hiệu hóa thanh soạn thảo đối với thành viên không có quyền nhắn tin.
- **Tối ưu hóa khởi động với lịch sử chat lớn ([#44](https://github.com/ctdoteam1110/ctdozlite/issues/44)):** Tăng tốc mở ứng dụng bằng cơ chế nạp lịch sử phân trang `PRELOAD_MSGS` thay vì nạp toàn bộ tin nhắn vào RAM.

## [0.0.7] - 2026-11-03

### Fixed (Sửa lỗi & Tinh chỉnh giao diện)
- **Sửa lỗi gõ tiếng Việt Telex trên macOS bị nuốt dấu cách & lỗi phím Enter ([#71](https://github.com/ctdoteam1110/ctdozlite/issues/71)):** Bảo toàn ký tự khoảng trắng (`" "`) trong sự kiện `Event::Ime(ImeEvent::Commit(" "))` của macOS NSTextInputClient/egui trước đây bị lọc nhầm bởi `.is_empty()`, đồng thời gắn cờ `ime_Preedit_active` để phím `Enter` chỉ xác nhận từ đang gõ dấu thay vì gửi tin nhắn sớm.
- **Hiển thị trực tiếp ảnh trong hội thoại & Trình xem ảnh tích hợp ([#72](https://github.com/ctdoteam1110/ctdozlite/issues/72)):** Phóng to khung hiển thị ảnh trực tiếp trong bong bóng chat lên tối đa `340x320`, tự động ẩn chuỗi URL CDN thô (`https://...zdn.vn/...`) khi đã có ảnh đính kèm, và chuyển toàn bộ thao tác bấm vào ảnh hoặc liên kết ảnh sang mở `ImageViewerModal` ngay trong ứng dụng (hỗ trợ phóng to/thu nhỏ, sao chép ảnh vào clipboard và lưu về máy).
- **Tự động kích hoạt đồng bộ tin nhắn từ điện thoại ngay khi đăng nhập ([#73](https://github.com/ctdoteam1110/ctdozlite/issues/73)):** Tự động khởi chạy đồng bộ lịch sử đám mây kèm gửi gói yêu cầu WebSocket `cmd 12000` & `cmd 12888` tới điện thoại ngay sau khi đăng nhập QR/Mật khẩu hoặc khôi phục phiên mà không cần bấm nút thủ công.
- **Tải tệp đính kèm trực tiếp & giữ nguyên tên file gốc của người gửi ([#74](https://github.com/ctdoteam1110/ctdozlite/issues/74)):** Sửa thứ tự trích xuất tên tệp trong `parse_chat_message` (`src/chat.rs`) ưu tiên trường `title` trước `href` để không bao giờ lấy nhầm đường dẫn endpoint `renewMedia.doc` của Zalo CDN, đồng thời tải trực tiếp tệp về thư mục `Downloads` với đúng tên gốc của người gửi thay vì mở trình duyệt ngoài.

## [0.0.6] - 2026-11-02

### Added (Tính năng mới & Cập nhật)
- **Trang Danh bạ chuẩn Zalo PC (`src/gui/modals.rs`, `src/gui/workspace.rs`):** Hợp nhất hai nút Cá nhân và Nhóm chat trên thanh điều hướng trái thành mục **Danh bạ** duy nhất với 4 phân mục chuẩn Zalo Desktop (*Danh sách bạn bè*, *Danh sách nhóm và cộng đồng*, *Lời mời kết bạn*, *Lời mời vào nhóm và cộng đồng*), hỗ trợ gom nhóm theo chữ cái đầu (`A–Z`), tìm kiếm và lọc nhanh.
- **Khôi phục gói nhãn dán động "Mèo Ami Bụng Bự (GIF)" (`src/gui/stickers.rs`, `src/assets/ami_fat_cat/`):** Tích hợp bộ 15 sticker GIF động trong bảng chọn nhãn dán và hỗ trợ gửi/hiển thị trực tiếp dưới dạng ảnh động mượt mà.
- **Hiển thị văn bản định dạng giàu (Markdown & Zalo RTF `textProperties`) (`src/chat.rs`, `src/gui/messages.rs`):** Giải mã định dạng `styles` trong `params.textProperties` của Zalo PC/Mobile và hỗ trợ hiển thị trực tiếp **in đậm**, *in nghiêng*, ~~gạch ngang~~, `inline code`, khối mã (`code block`), trích dẫn (`blockquote`), tiêu đề và danh sách ngay trong bong bóng tin nhắn.
- **Đồng bộ trạng thái tắt thông báo (Mute) hai chiều ([#67](https://github.com/ctdoteam1110/ctdozlite/issues/67)), Đồng bộ tin nhắn từ điện thoại ([#68](https://github.com/ctdoteam1110/ctdozlite/issues/68)) & Hỗ trợ Đa tài khoản / Proxy ([#64](https://github.com/ctdoteam1110/ctdozlite/issues/64)).**

### Fixed (Sửa lỗi & Tinh chỉnh giao diện)
- **Cải tiến khung nhập tin nhắn đa dòng (`src/gui/workspace.rs`):** Tự động mở rộng chiều cao từ 1 lên tối đa 5 dòng (`34px` → `114px`) khi nhấn `Shift + Enter`, từ dòng thứ 6 trở đi tự cuộn dọc bên trong và khóa cứng chiều ngang để không bao giờ đẩy lệch cụm nút `🕒` và `Gửi`.
- **Thiết kế lại hộp thoại Hẹn giờ gửi tin nhắn & Nhắc hẹn (`src/gui/modals.rs`):** Chuyển sang thẻ nổi bo góc hiện đại có màn mờ nền (`backdrop`), lưới 4 nút chọn nhanh cân đối và băng thông báo vàng hổ phách độ tương phản cao.

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
