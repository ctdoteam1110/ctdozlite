# Bàn giao dự án QTzalo — từ v0.1.0 sang v0.1.1

> Tài liệu này dành cho phiên làm việc (người hoặc AI) tiếp theo. Đọc hết trước khi sửa code.
> Cập nhật lần cuối: 2026-11-01, sau khi phát hành v0.1.0.

## 1. Trạng thái hiện tại

- **Repo:** `quangthoai1985/QTzalo`, bản fork của `ctdoteam1110/ctdozlite` (tách từ commit `73849bd` = tag upstream `v0.0.2`).
- **`main` = `47c16f9`.** Đã phát hành [QTzalo v0.1.0](https://github.com/quangthoai1985/QTzalo/releases/tag/v0.1.0) do CI build. Release gồm:
  - bộ cài và bản portable Windows
  - `.deb` và `.tar.gz` cho Linux
  - `.dmg` cho macOS
  - `SHA256SUMS.txt`
- **GitHub Actions đã bật.** Workflow duy nhất: `.github/workflows/ci-release.yml`. Cả 6 lần chạy đều xanh.
- **Kiểm chứng cục bộ của v0.1.0:**
  - `cargo test --locked`: 123 test pass, 1 bỏ qua.
  - `cargo check --target x86_64-pc-windows-gnu`: sạch.
  - `cargo audit`: 0 lỗ hổng, 5 cảnh báo đã biết (xem §8).
- **Nhật ký thay đổi và trạng thái từng lỗi:**
  - `CHANGELOG.md`
  - `docs/SECURITY_AUDIT.md` (bảng S1–S14, B1–B9 kèm commit sửa)
  - `docs/CAI_DAT.md`
  - `README.md`

## 2. Quyết định đã được người dùng chốt

| Chủ đề | Quyết định |
|---|---|
| Tên sản phẩm | **QTzalo**. Cargo package/binary vẫn là `ctdozlite` / `zalo-rust`. Bộ cài Windows cài file chạy thành `QTzalo.exe`. |
| Nhánh & phát hành | **Được merge vào `main` và phát hành Release từ `main`** bằng fast-forward, không viết lại lịch sử. Người dùng đã đồng ý lại cho v0.1.1. |
| Thư mục dữ liệu | `%APPDATA%\QTzalo` / `~/.local/share/qtzalo` / `~/Library/Application Support/QTzalo` (`src/paths.rs`). Biến `QTZALO_DATA_DIR` ghi đè. |
| Giao diện | 3 cột tông sáng theo ảnh mẫu của người dùng; cột trái và cột hội thoại thu gọn được còn icon/avatar; vẫn có giao diện tối. |
| Phạm vi v0.1.1 | Làm các chức năng còn thiếu ở §5 rồi **phát hành v0.1.1 luôn**. |
| Khi nào hỏi người dùng | Chỉ dừng lại hỏi khi cần một quyết định thật sự (ví dụ: đổi tên binary, xoá dữ liệu người dùng, thay đổi hành vi bảo mật lớn). |

## 3. Bản đồ mã nguồn

| Khu vực | File | Ghi chú |
|---|---|---|
| Khởi động, CLI | `src/main.rs` | `--gui` (mặc định), `--tui`, `--qr`, `--dev`, `--demo`, `--logout`, `--pass-stdin` |
| Đường dẫn dữ liệu | `src/paths.rs` | `data_dir()`, `session_file()`, `history_db()`, `cache_dir()`, `logs_dir()`, `settings_file()`, `write_private()` (ghi nguyên tử, 0600), `migrate_legacy_files()` |
| Định dạng giờ | `src/timefmt.rs` | `hm_from_ms`, `local_day_index`, `local_weekday`… dùng `chrono::Local` |
| Phiên & HTTP | `src/session.rs` | `SessionStatus {Valid, Expired, Unreachable}`, `check_session()`, domain đã kiểm chứng thắng `zpw_service_map_v3`, chỉ gửi cookie tới host https của Zalo |
| Đăng nhập | `src/login.rs`, `src/qr.rs`, `src/param_cipher.rs`, `src/crypto.rs` | ParamCipher v2, sinh số ngẫu nhiên bằng `getrandom` |
| Tin nhắn / API | `src/chat.rs` (lớn, khoảng 4,6k dòng) | `get_friends`, `get_groups` (`/api/group/getmg-v2`), `get_group_history`, upload ảnh/tệp, `parse_link_metadata(s, url, allow_direct_fetch)` |
| WebSocket | `src/socket.rs` | chỉ dùng `wss://` của Zalo; giải nén giới hạn 32 MiB |
| Cuộc gọi | `src/call.rs`, `src/media.rs`, `src/voip_log.rs`, `src/audio.rs` | log chỉ ghi khi `--dev` / `ZALO_CALL_TRACE=1` / `ZALO_VOIP_LOG` |
| Lưu lịch sử | `src/store.rs` | SQLite với truy vấn tham số hoá |
| GUI — vòng lặp & sự kiện | `src/gui.rs` | `enum GuiEvent`, `AppSettings` (gồm `download_dir`), `ZaloGuiApp`, `is_demo_mode()` |
| GUI — worker nền | `src/gui/worker.rs` | `enum WorkerCmd`; mọi việc mạng chạy ở đây (tokio) |
| GUI — bố cục | `src/gui/sidebar.rs` (thanh điều hướng + danh sách hội thoại), `src/gui/workspace.rs` (khung chat, composer), `src/gui/messages.rs` (bong bóng, thẻ link/ảnh/tệp, mở URL, file picker, clipboard), `src/gui/modals.rs` (Cài đặt 7 tab, trình xem ảnh, cuộc gọi…), `src/gui/stickers.rs`, `src/gui/theme.rs` (bảng màu, dò font), `src/gui/demo.rs` (dữ liệu mẫu offline) | |
| TUI | `src/tui.rs`, `src/ui.rs` | |
| Công cụ dev | `src/bin/*` | `call_probe`, `test_fetch`, … |

## 4. Build, test, kiểm chứng

```bash
# Rust >= 1.88 (1.87 bị cargo từ chối vì cookie_store/darling/image/time). Stable mới nhất đều chạy được.
export CMAKE_POLICY_VERSION_MINIMUM=3.5   # cần khi CMake >= 4 (audiopus_sys)
cargo build --locked --bin zalo-rust
cargo test --locked                        # phải xanh
bash scripts/check-windows.sh              # cargo check cho x86_64-pc-windows-gnu (cài target + mingw nếu thiếu)
cargo audit                                # đã cài sẵn trong môi trường cloud trước; nếu thiếu: cargo install cargo-audit --locked
```

- **Thư viện hệ thống cần trên Linux:** `build-essential cmake pkg-config libasound2-dev libxkbcommon-dev libxcb-shape0-dev libxcb-xfixes0-dev`. Để chụp ảnh giao diện cần thêm `libgl1-mesa-dri libegl1 libxkbcommon-x11-0 libxcursor1 libxrandr2 libxi6 fonts-noto-core fonts-noto-cjk xvfb imagemagick`.
- **Chụp ảnh giao diện không cần tài khoản thật** (chế độ demo không mở mạng và không ghi dữ liệu):

  ```bash
  HOME=$(mktemp -d) xvfb-run -a -s "-screen 0 1280x800x24" sh -c \
    "./target/debug/zalo-rust --gui --demo & sleep 12; import -window root /tmp/shot.png; kill %1"
  ```

  Các biến `QTZALO_DEMO_*` trong `src/gui/demo.rs` / `src/gui.rs` chỉnh kích thước, theme, trạng thái thu gọn và hội thoại được chọn. Xem ảnh bằng công cụ đọc ảnh trước khi kết luận giao diện đã đúng.

## 5. Phạm vi v0.1.1

### P0 — bắt buộc

1. **Tải tệp/media về "Thư mục lưu file"**
   - **Hiện trạng:** `AppSettings.download_dir` (mặc định `%USERPROFILE%\Downloads\Zalo Received Files`) chỉ được lưu và mở bằng nút "Mở thư mục" (`src/gui.rs:262`, `src/gui/modals.rs:1370-1376`). Bấm vào tệp, video hay ảnh gốc chỉ gọi `open_external_url` mở trình duyệt (`src/gui/messages.rs:281`, các chỗ gọi ở `:1158 :1243 :1274 :1298 :1312`).
   - **Cần làm:**
     - Nút **"Tải về"** trên thẻ tệp, thẻ video và thẻ ảnh (`src/gui/messages.rs`, khoảng `:1342-1420`), và trong thanh công cụ trình xem ảnh (`src/gui/modals.rs`, khoảng `:2549-2627`).
     - Khi tải xong, toast có nút **"Mở"** và **"Mở thư mục"**.
     - Có trạng thái đang tải (phần trăm nếu biết `Content-Length`) và báo lỗi rõ ràng.
   - **Worker:** thêm `WorkerCmd::DownloadAttachment { url, suggested_name, thread_id }` cùng các `GuiEvent::DownloadProgress / DownloadFinished { path } / DownloadFailed`. Tải theo luồng (stream) xuống đĩa, không giữ cả file trong RAM.
   - **Yêu cầu bảo mật (bắt buộc):**
     - Chỉ tải qua `https://`; `http://` chỉ cho host CDN Zalo (`*.zdn.vn`, `*.zalo.me`, `*.zaloapp.com`) như chính sách ảnh hiện có trong `worker.rs`.
     - **Làm sạch tên file:** bỏ `/ \ : * ? " < > |`, ký tự điều khiển, dấu chấm và khoảng trắng ở cuối; đổi tên dành riêng của Windows (`CON`, `PRN`, `AUX`, `NUL`, `COM1-9`, `LPT1-9`); giới hạn độ dài khoảng 150 ký tự; tên rỗng thì đặt `tep_<thời gian>`.
     - **Không ghi đè file có sẵn:** thêm hậu tố `" (1)"`, `" (2)"`…
     - Ghi vào `*.part` rồi đổi tên. Thư mục đích phải là đường dẫn tuyệt đối; tạo thư mục nếu chưa có.
     - Trên Windows, ghi ADS `:Zone.Identifier` với `ZoneId=3` (Mark-of-the-Web) để SmartScreen/Office cảnh báo đúng.
     - **Không tự mở file thực thi.** Với `.exe .msi .bat .cmd .com .scr .ps1 .vbs .js .jse .wsf .hta .lnk .reg .jar .cpl .dll`, nút "Mở" chỉ mở thư mục (hoặc hỏi xác nhận rõ ràng).
     - Tên file và URL lấy từ tin nhắn của người khác là **dữ liệu không tin cậy**.
   - **Unit test:** làm sạch tên file, đặt tên tránh trùng, chính sách URL. Logic thuần nên tách khỏi I/O.
2. **Sửa ảnh dán từ clipboard ghi ra thư mục hiện hành:** `src/gui/messages.rs:443-448` vẫn dùng `"stickers_cache/clipboard_{ms}.png"` tương đối. Chuyển sang `crate::paths::cache_dir().join(...)`, và xoá các `clipboard_*.png` cũ hơn 24 giờ khi khởi động.
3. **Quản lý cache ảnh trong Cài đặt** (tab "Dữ liệu" hoặc tab phù hợp trong `src/gui/modals.rs`):
   - Hiển thị dung lượng `paths::cache_dir()`; tính ở worker hoặc thread phụ để không chặn khung hình.
   - Nút **"Xoá cache ảnh"**: chỉ xoá nội dung `stickers_cache/`, **không động** tới `session.json`, `zalo_history.db`, `app_settings.json`. Có hộp xác nhận. Xoá xong thì dọn texture trong RAM để ảnh tự tải lại.
   - Hiển thị đường dẫn thư mục dữ liệu, kèm nút "Mở thư mục dữ liệu".

### P1 — nên làm nếu khả thi (mỗi mục phải có cách kiểm chứng; không bịa dữ liệu giao thức)

4. **Số thành viên nhóm thật** ở header chat: hiện chỉ có ở demo, chế độ thật hiện "Nhóm". `chat::get_groups` đã gọi `/api/group/getmg-v2` (`src/chat.rs` khoảng `:2728-2760`). Kiểm tra payload có `totalMember` / `memVerList` (xem `re-notes/02_chat_data.md`), thêm trường `member_count: Option<u32>` vào `Thread` nếu dữ liệu có thật. Không có thì giữ "Nhóm".
5. **Chỉ báo "đang nhập" thật:** GUI đã có `typing_indicators` (`src/gui/workspace.rs` khoảng `:1207`, `:1569`) nhưng worker chưa phát sự kiện. Chỉ làm nếu tìm thấy bằng chứng sự kiện typing trong luồng WebSocket/long-poll (`src/socket.rs`, `re-notes/`). Nếu không chắc thì ghi vào tồn đọng.
6. **Dependency:** thử nâng `ratatui` (0.29 kéo `lru 0.12.5` bị đánh dấu unsound) nếu API tương thích và TUI vẫn build/chạy. Cập nhật các dependency patch khác khi `cargo audit` sạch và MSRV ≤ 1.88.

### P2 — chỉ làm khi P0/P1 xong và còn thời gian

7. **Đồng bộ lịch sử có giới hạn (#19):** nút "Tải thêm tin nhắn cũ" đã có (`WorkerCmd::LoadGroupHistory`). Có thể mở rộng phân trang `getoldv2` với tiến trình và nút huỷ, nhưng phải thiết kế trước. Nếu không làm, giữ nguyên trong tồn đọng.

### Không làm trong v0.1.1

- Lọc gói UDP theo relay (S6): cần thử với cuộc gọi thật.
- Ký số bộ cài.
- Đổi tên binary.

## 6. Quy trình phát hành (đã chạy thành công cho v0.1.0)

1. Sửa `Cargo.toml` thành `version = "0.1.1"`, chạy `cargo build` để `Cargo.lock` cập nhật dòng `ctdozlite`.
2. `CHANGELOG.md`: thêm mục `## [0.1.1] - <ngày>` (Thêm mới / Sửa lỗi / Bảo mật) và link `[0.1.1]` ở cuối file. Cập nhật `docs/SECURITY_AUDIT.md` nếu có thay đổi bảo mật.
3. `.github/release-notes.md`: cập nhật mục "Điểm mới" cho 0.1.1. Template có sẵn `{{VERSION}}` / `{{COMMIT}}`.
4. Commit theo Conventional Commits, kết thúc bằng hai dòng attribution mà hệ thống cung cấp cho phiên. Push nhánh làm việc; CI chạy cả bản Windows trên mọi push vào `claude/**`.
5. Khi CI xanh: fast-forward `main` (`git push origin HEAD:main`; trước đó kiểm tra `git merge-base --is-ancestor origin/main HEAD`).
6. Kích hoạt phát hành bằng GitHub MCP:
   - `actions_run_trigger` với `method=run_workflow`, `workflow_id=ci-release.yml`, `ref=main`, `inputs={"release": true}`.
   - Job `release` tự tạo tag `v<version>` tại commit đã build và chỉ chạy khi `test` + `windows` xanh.
   - Release đã tồn tại thì job dừng có chủ ý, nên mỗi lần phát hành phải tăng phiên bản.
7. Kiểm tra bằng `get_release_by_tag v0.1.1` rằng release có đủ file:
   - `QTzalo-0.1.1-setup-windows-x86_64.exe`
   - `QTzalo-0.1.1-portable-windows-x86_64.exe`
   - `qtzalo_0.1.1_amd64.deb`
   - `qtzalo-0.1.1-linux-x86_64.tar.gz`
   - `QTzalo-0.1.1-macos-arm64.dmg` (macOS là best-effort)
   - `SHA256SUMS.txt`

## 7. Bài học khi điều phối nhiều agent

- **Tạo trước "điểm nối" dùng chung rồi mới giao việc.** Ví dụ ở v0.1.0: `paths.rs`, `timefmt.rs`. Với v0.1.1, coordinator nên tự thêm các biến thể `WorkerCmd`/`GuiEvent` cho tải file cùng một module thuần (ví dụ `src/download.rs` chứa làm sạch tên file, đặt tên tránh trùng, chính sách URL, có test), commit và push, rồi mới giao. Khi đó agent GUI và agent worker không sửa chồng lên nhau.
- **Phân quyền file rõ ràng cho từng agent.** Ghi rõ được sửa file nào và cấm sửa file nào. Agent cần thứ ngoài quyền thì ghi vào báo cáo.
- **Worktree có thể được tạo từ `main` cũ, không phải commit nền vừa push.** Sau khi giao việc, chạy `git worktree list`; nếu sai, nhắn agent `git stash && git merge --ff-only <commit nền> && git stash pop` (hoặc `git rebase <commit nền>` nếu đã commit).
- **Đĩa có hạn (khoảng 25 GB trống lúc bắt đầu).** Mỗi thư mục `target` debug khoảng 3,5 GB. Không để mỗi agent copy riêng một thư mục `target`: dùng chung, dùng `cargo check` nhiều hơn `cargo build`, và xoá worktree đã merge (`git worktree remove --force --force …`). Nếu đầy đĩa, xoá `target/*/incremental` và `target/x86_64-pc-windows-gnu`.
- **Agent có thể dừng vì chạm giới hạn sử dụng.** Phần việc vẫn được lưu; khi giới hạn được đặt lại, gửi `SendMessage` để agent làm tiếp, không tạo agent mới.
- **Chế độ tự động chặn việc chạy code bên ngoài chưa review** (ví dụ chạy test trên nhánh PR của upstream). Khi đó đọc diff tĩnh rồi tự viết lại.
- **Hook dừng phiên báo file untracked `.claude/`** (thư mục worktree). Thêm `.claude/` vào `.git/info/exclude`; không commit thư mục này.
- **Không dùng `gh` CLI cho việc khác ngoài tìm kiếm.** Mọi thao tác GitHub (Actions, Release, PR) dùng GitHub MCP. CI chạy được `gh` trên runner, điều này không bị ảnh hưởng.
- **Không thể ngủ chờ (sleep/poll).** Để chờ CI, dùng `send_later` hẹn tự kiểm tra lại sau khoảng 10–12 phút. Run Windows mất khoảng 9–10 phút.
- **Không ghi model identifier** vào commit, PR hay mã nguồn.

## 8. Tồn đọng đã biết

- **S6:** media cuộc gọi chưa lọc nguồn gói UDP; client khai `srtpMode: 0`.
- **#19:** chưa đồng bộ toàn bộ lịch sử.
- **Cảnh báo `cargo audit`:** `lru 0.12.5` unsound (qua ratatui 0.29); `audiopus_sys`, `paste`, `ttf-parser` hết bảo trì.
- **Chế độ thật:** header nhóm hiện "Nhóm" thay vì số thành viên; chỉ báo đang nhập chỉ có ở demo.
- **Emoji màu:** egui không vẽ được emoji màu (đã thêm font Segoe UI Emoji làm dự phòng nét đơn trên Windows, chưa thử trên máy thật).
- **Bộ cài chưa ký số**, nên SmartScreen cảnh báo.
- **Chưa thử với tài khoản Zalo thật** sau khi sửa các lỗi lõi #21/#22. Nên nhờ người dùng thử và báo lại.
