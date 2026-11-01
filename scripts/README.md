# Developer & Research Scripts

| Script | Mục đích |
|---|---|
| `check-windows.sh` | Kiểm tra (hoặc build với `--build`) bản Windows ngay trên Linux bằng MinGW-w64 (`x86_64-pc-windows-gnu`). Tự cài target Rust và `mingw-w64` nếu thiếu. Bản phát hành chính thức vẫn build bằng MSVC trên CI. |
| `decode_voip_log.py` | Giải mã/tóm tắt file `voip.log` (chỉ có khi chạy với `--dev`). |
| `grab.py` | Trích xuất và lọc các gói tin Zalo quan trọng từ file ghi lưu lượng `zalo.flow` (mitmproxy). |
| `test_pc_status.py` | Kiểm tra nhanh trạng thái phản hồi của endpoint xác nhận QR `pc-status` từ `zalo.flow`. |

File capture (`*.flow`) và log chứa dữ liệu phiên thật: không commit lên repo.
