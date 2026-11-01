# Đóng góp tính năng hội thoại

## Phạm vi

Đóng góp bao gồm cả thay đổi UI cần cho chức năng. **UI chưa được tối ưu/hoàn thiện về mặt thiết kế**, không phải loại UI khỏi PR. Chỉ đã kiểm thử trên Mac, cụ thể macOS arm64. Windows, Linux, Mac Intel/Universal chưa được nghiệm thu. PR là đề xuất để maintainer review, không phải release đã chứng nhận.

### Chức năng được đưa lên

- Emoji trong bubble và sidebar: alias longest-match/boundary, Unicode grapheme, skin tone/ZWJ/flag/keycap, raw selection/copy, native color fallback, Noto Emoji/Math OFL và renderer sprite opt-in local.
- Media: parse nhiều dạng metadata, ảnh/video thumbnail, photo source fallback khi thumbnail lỗi, không dùng video source làm ảnh, giới hạn input/pixel/cache/queue/retry, native ImageIO preview và viewer ảnh/video. URL ký giữ nguyên bytes; media hết hạn vẫn có thể cần server cấp lại.
- Lịch sử 1-1 và nhóm: socket pull, import/sync từ điện thoại qua helper codec, lịch sử có pagination và giới hạn cửa sổ. Không ép điện thoại đồng bộ khi người dùng không yêu cầu.
- Reaction: baseline lịch sử có provenance, journal theo tài khoản, deduplicate và replay đúng target ID, bảo toàn aggregate từ người khác.
- Đọc/chưa đọc và receipt: parse trạng thái từ server, manual unread/mention/mute, badge và tên in đậm, chỉ read nội dung hiển thị khi foreground đủ dwell, local read bền qua restart và không phụ thuộc network ACK.
- Điều hướng: bấm reply để tìm/cuộn tới tin gốc, mention clickable với offset UTF-16, tên người gửi cuối, avatar collage thành viên khi nhóm không có avatar, ngày ở sidebar khi quá 24 giờ và presence không tự mặc định vừa truy cập.
- Persistence/runtime: Application Support, `--data-dir`, session private/atomic, kho V2 theo tài khoản, import legacy giữ nguồn cũ, identity reconciliation, generation/cancellation, FIFO persistence, đường đóng app qua eframe và không tự giành foreground.
- Native notification/helper, attachment picker và synthetic storage benchmark.

PR thời gian và data directory độc lập vẫn có thể merge trước. Nhánh tổng hợp giữ phần phụ thuộc của chúng để build được trên upstream v0.0.3 chưa có hai fix đó; maintainer cần tránh merge trùng bằng rebase sau khi các PR nhỏ vào main.

## Build trên Mac từ checkout sạch

```sh
# Requirements: Rust/Cargo, Xcode command line tools, static zstd.
# Pillow is needed only for the native Python tests.
export CMAKE_POLICY_VERSION_MINIMUM=3.5
cargo test --locked
bash native/macos/build-native-helpers.sh
cargo build --release --locked --bin zalo-rust
```

Bốn helper host được build: `EmojiPreview`, `MediaPreview`, `DailyMediaViewer`, `DailyNotifier.app`. Các builder chỉ ghi vào `native/macos/build`, không thay app trong Applications.

Phone history codec là bước opt-in riêng vì có dependency assets bên ngoài:

```sh
# Downloads and validates pinned public codec bytes locally, never account data.
export ZSTD_STATIC_LIBRARY="$(brew --prefix zstd)/lib/libzstd.a"
WITH_HISTORY_CODEC=1 bash native/macos/build-native-helpers.sh
python3 native/macos/test-history-codec.py
```

Đóng gói thử cho **cùng kiến trúc host**:

```sh
APP=/tmp/chat-feature-test/ctdozlite.app
mkdir -p "$APP/Contents/MacOS"
cp target/release/zalo-rust "$APP/Contents/MacOS/"
cp packaging/macos/Info.plist "$APP/Contents/Info.plist"
bash native/macos/build-native-helpers.sh "$APP"
# Only after an explicit local codec preparation:
WITH_HISTORY_CODEC=1 bash native/macos/build-native-helpers.sh "$APP"
codesign --force --deep --sign - "$APP"
```

Không dùng host helpers để tuyên bố bundle Intel/Universal chạy được. Workflow release mới gắn builder EmojiPreview; MediaPreview, DailyMediaViewer, DailyNotifier và HistoryCodec chưa được gắn vào đầy đủ helper/codec matrix; cần hoàn thành và kiểm trên máy đích trước release. Việc compile Rust không thay thế kiểm runtime codec/viewer.

## Kiểm tra

```sh
cargo test --locked
python3 native/macos/test-emoji-preview.py
python3 native/macos/test-media-preview.py
python3 native/macos/test-history-codec.py
native/macos/build/DailyMediaViewer --self-test
native/macos/build/DailyNotifier.app/Contents/MacOS/DailyNotifier --self-test
cargo run --release --locked --bin daily_parity_bench -- \
  --messages 5000 --switches 200 --soak-seconds 2 --output /tmp/chat-feature-metrics.json
```

Benchmark chỉ mô phỏng storage/window, không điều khiển GUI, không phải chứng minh 60 phút app không crash/RAM tăng. Các regression tests không đọc tài khoản thật; một số dùng mock loopback cục bộ, không gọi server Zalo.

Manual checklist sau human audit: chọn data-dir dùng riêng, đăng nhập; so emoji/raw copy; mở ảnh/video mới/cũ; đồng bộ 1-1/nhóm khi điện thoại rảnh; kiểm reaction aggregate; mở chat unread với foreground rồi restart; reply/mention jump; đổi tài khoản không lẫn dữ liệu. Không đăng log/capture/account data lên PR.

## Rủi ro đang mở

- Shutdown hiện chỉ enqueue snapshot cuối theo best-effort, chưa có worker flush ACK/join barrier. Không đảm bảo delta cuối đã durable khi tiến trình thoát ngay; cần shutdown handshake trước merge.
- V2 cần review và chuẩn hóa schema migrations theo `user_version`, policy WAL và newer-version guard trước merge. Hiện chưa down-migration.
- Local wipe đang khóa có chủ đích để không xóa nửa vời hoặc tái import legacy. Cần account generation/cancellation fence trước khi bật lại.
- Emoji artwork độc quyền không được commit. 88 sprite alias không có Unicode tương đương giữ raw text khi chưa có pack local, không đoán biểu tượng khác.
- Native/phone codec, Web assets và server protocol có thể đổi; CI/release cross-platform chưa nghiệm thu. Không khẳng định parity hoàn toàn với Zalo gốc.
- UI có chức năng mới nhưng chưa tối ưu thiết kế và chưa có accessibility/visual QA toàn diện.

Xem [ADR](adr/0001-account-scoped-chat-state.md) và [protocol evidence map](../re-notes/09_chat_state_compatibility.md). Rollback bằng revert source trên nhánh, giữ backup dữ liệu riêng và legacy, không tự xóa cache/schema/tài khoản để che lỗi.
