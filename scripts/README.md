# Developer & Research Scripts

Thư mục này chứa các kịch bản hỗ trợ kiểm thử giao thức và phân tích lưu lượng mạng:

- `grab.py`: Script trích xuất và lọc các gói tin Zalo quan trọng từ file ghi lưu lượng `zalo.flow` (mitmproxy).
- `test_pc_status.py`: Script kiểm tra nhanh trạng thái phản hồi của endpoint xác nhận QR `pc-status`.
- `build_twemoji_pack.py`: Gói ảnh Twemoji 72x72 (https://github.com/jdecked/twemoji, đồ hoạ CC-BY 4.0) thành `src/assets/emoji/twemoji72.pack`, file mà `src/gui/color_emoji.rs` dùng để vẽ emoji màu. Chạy lại khi nâng phiên bản Twemoji: `python3 scripts/build_twemoji_pack.py <twemoji>/assets/72x72 src/assets/emoji/twemoji72.pack`.
