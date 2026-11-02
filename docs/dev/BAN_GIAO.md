# Bàn giao dự án QTzalo — từ v0.1.2 sang bản tiếp theo

> Tài liệu này dành cho phiên làm việc (người hoặc AI) tiếp theo. Đọc hết trước khi sửa code.
> Cập nhật lần cuối: 2026-11-02, khi phát hành v0.1.2. Mục 0 là phần mới; mục 1 trở đi giữ nội dung bàn giao của 0.1.1 và được sửa ở những chỗ đã thay đổi. Bản bàn giao trước xem lịch sử git của file này.

## 0. v0.1.2 (2026-11-02) — tóm tắt cho phiên sau

- **Phiên này chạy trên PC Windows của người dùng**, với tài khoản Zalo thật và điện thoại thật.
- **Nhánh `claude/v0.1.2`** được fast-forward vào `main`; Release `v0.1.2` do CI build.
- **Đã làm:**
  - Phần A của `docs/dev/KE_HOACH_TIEP_THEO.md`: con trỏ hàng đợi bền theo tài khoản, kéo bù tới khi hết, banner thiếu tin, nối lịch sử Cloud/nhóm.
  - Danh sách hội thoại giống điện thoại (A9).
  - Ảnh/video/tệp không còn hiện link; cửa sổ xem ảnh và cửa sổ phát video riêng (A8, do agent làm).
  - Sửa tên nhóm hiện thành "Nhóm <số>"; sửa lỗi long-poll lặp ở queue `16`.
  - Chi tiết: `CHANGELOG.md` `[0.1.2]`.
- **Bằng chứng thật:** `re-notes/10_bang_chung_dong_bo.md`, gồm các lượt A0, A7 và ngày 02/11. Trace thô nằm ở `%USERPROFILE%\qtzalo-trace\`, **không commit**.
- **Kiểm chứng của 0.1.2:**
  - `cargo test --locked`: 191 qua, 1 bỏ qua.
  - Build MSVC trên PC sạch; `cargo +1.88 check --locked --all-targets` sạch.
  - `cargo audit`: 0 lỗ hổng, 2 cảnh báo hết bảo trì.
  - Kịch bản thật:
    - con trỏ đã lưu: không có tin mới → 0 tin, có 3 tin mới → đúng 3;
    - một ngày nhiều tin 1-1 và nhóm: kéo đủ theo trang;
    - 61/61 nhóm đúng tên;
    - người dùng thử và xác nhận ảnh, video (kể cả video cũ trên `dlmd.me`) và tệp mở trong app.
  - Ảnh `--demo` đã xem: `docs/assets/qtzalo-sync.png` (sáng), bản tối trong scratchpad, `qtzalo-media-cards.png`, `qtzalo-image-window.png`, `qtzalo-video-window.png`.
- **Chưa kiểm chứng:**
  - Banner thiếu tin với `evict` thật (chưa gặp);
  - tắt máy lâu hơn một ngày;
  - Linux/macOS;
  - tiếng video chỉ được người dùng xác nhận gián tiếp ("Chạy tốt").
- **Phát hiện quan trọng:**
  1. Phiên QTzalo nhận queue `510_1, 511_1, 515_0, 517_0, 518_0` (không phải `510_0/513_0`).
  2. Kéo về không xoá tin khỏi hàng đợi.
  3. Long-poll phải theo dõi `15/16` nếu máy chủ báo.
  4. Thiết bị mới không lấy được lịch sử nhóm trước lần đăng nhập đầu tiên.
  5. Không nhóm nào E2EE.
  6. Có sự kiện `transfer_after_login` mang `temp_key` (liên quan Phần B).
- **Việc tiếp theo:** Phần B (đồng bộ từ điện thoại, hướng Sync V1), chỉ khi người dùng đồng ý. Đọc `re-notes/09` §9 và `KE_HOACH_TIEP_THEO.md` §2 (đã thêm bước phân tích APK Android và câu hỏi B6.6–B6.7).

## 1. Trạng thái của v0.1.1 (giữ lại để tham chiếu)

- **Repo:** `quangthoai1985/QTzalo`, bản fork của `ctdoteam1110/ctdozlite` (tách từ commit `73849bd` = tag upstream `v0.0.2`).
- **`main` = commit phát hành 0.1.1** (fast-forward từ nhánh `claude/release-0.1.1`). Release [QTzalo v0.1.1](https://github.com/quangthoai1985/QTzalo/releases/tag/v0.1.1) do CI build (bộ cài + portable Windows, `.deb`, `.tar.gz`, `.dmg`, `SHA256SUMS.txt`). v0.1.0 vẫn còn trên trang Releases.
- **Đã làm trong 0.1.1** (chi tiết: `CHANGELOG.md`):
  - Tải tệp/video/ảnh về "Thư mục lưu file" — `src/download.rs` (chính sách + I/O), `WorkerCmd::DownloadAttachment`, `GuiEvent::Download{Progress,Finished,Failed}`, `DownloadState` và `downloads`/`download_toast` trong `ZaloGuiApp`; nút trên thẻ (`src/gui/messages.rs`, `BubbleAction::{Download,OpenDownloaded,RevealDownloaded}`), trình xem ảnh và toast (`src/gui/modals.rs`).
  - Quản lý cache ảnh trong *Cài đặt → Dữ liệu & làm mới* (`WorkerCmd::{ComputeCacheSize,ClearImageCache}`), dọn `clipboard_*.png` > 24 h khi khởi động (`paths::cleanup_old_clipboard_images`).
  - Ảnh dán clipboard ghi vào `paths::cache_dir()`; chọn thư mục lưu file bằng hộp chọn thư mục (`pick_native_folder_dialog`).
  - Số thành viên nhóm thật từ `totalMember` của `getmg-v2` (cache trong tiến trình `chat::group_member_count`, **không** lưu vào `Thread`/`threads_cache.json`).
  - ratatui 0.30 / crossterm 0.29 (bỏ `lru` unsound và `paste`) + vài bản patch.
- **Kiểm chứng cục bộ của 0.1.1:** `cargo test --locked` 147 pass / 1 bỏ qua; `scripts/check-windows.sh` sạch; `cargo +1.88 check --locked --all-targets` sạch; `cargo audit` 0 lỗ hổng, 2 cảnh báo hết bảo trì; ảnh giao diện `--demo` (sáng/tối, Cài đặt) đã xem — `docs/assets/qtzalo-download.png`.
- **CHƯA kiểm chứng:** chưa chạy trên Windows thật (MotW, `explorer /select`, hộp chọn thư mục PowerShell), chưa thử với tài khoản Zalo thật (tải file thật từ CDN Zalo, xoá cache thật, `totalMember` có thật trong payload hay không).
- **Sau phát hành 0.1.1:** đã nghiên cứu cơ chế đồng bộ của Zalo PC (`re-notes/09_dong_bo_tin_nhan.md`) và lập kế hoạch tiếp theo (`docs/dev/KE_HOACH_TIEP_THEO.md`). Chưa có thay đổi mã nguồn nào sau `d8ba11c`.
- **Nhật ký thay đổi và trạng thái từng lỗi:** `CHANGELOG.md`, `docs/SECURITY_AUDIT.md` (S1–S16, B1–B9), `docs/CAI_DAT.md`, `README.md`.

## 2. Quyết định đã được người dùng chốt

| Chủ đề | Quyết định |
|---|---|
| Tên sản phẩm | **QTzalo**. Cargo package/binary vẫn là `ctdozlite` / `zalo-rust`. Bộ cài Windows cài file chạy thành `QTzalo.exe`. |
| Nhánh & phát hành | **Được merge vào `main` và phát hành Release từ `main`** bằng fast-forward, không viết lại lịch sử. Người dùng đã đồng ý lại cho v0.1.1. |
| Thư mục dữ liệu | `%APPDATA%\QTzalo` / `~/.local/share/qtzalo` / `~/Library/Application Support/QTzalo` (`src/paths.rs`). Biến `QTZALO_DATA_DIR` ghi đè. |
| Giao diện | 3 cột tông sáng theo ảnh mẫu của người dùng; cột trái và cột hội thoại thu gọn được còn icon/avatar; vẫn có giao diện tối. |
| Phạm vi v0.1.1 | Đã làm và phát hành (P0 đủ, P1: số thành viên + dependency; chỉ báo đang nhập chưa làm vì thiếu bằng chứng giao thức). |
| Khi nào hỏi người dùng | Chỉ dừng lại hỏi khi cần một quyết định thật sự (ví dụ: đổi tên binary, xoá dữ liệu người dùng, thay đổi hành vi bảo mật lớn). |

## 3. Bản đồ mã nguồn

| Khu vực | File | Ghi chú |
|---|---|---|
| Khởi động, CLI | `src/main.rs` | `--gui` (mặc định), `--tui`, `--qr`, `--dev`, `--demo`, `--logout`, `--pass-stdin` |
| Đường dẫn dữ liệu | `src/paths.rs` | `data_dir()`, `session_file()`, `history_db()`, `cache_dir()`, `logs_dir()`, `settings_file()`, `write_private()` (ghi nguyên tử, 0600), `migrate_legacy_files()` |
| Tải tệp | `src/download.rs` | `download_url_allowed`, `sanitize_filename`, `pick_unique_path`, `download_to_dir` (stream → `.part` → hard link không ghi đè), `write_mark_of_the_web`, `open_downloaded` (không mở file thực thi), `dir_size`, `clear_dir_contents` |
| Định dạng giờ | `src/timefmt.rs` | `hm_from_ms`, `local_day_index`, `local_weekday`… dùng `chrono::Local` |
| Phiên & HTTP | `src/session.rs` | `SessionStatus {Valid, Expired, Unreachable}`, `check_session()`, domain đã kiểm chứng thắng `zpw_service_map_v3`, chỉ gửi cookie tới host https của Zalo |
| Đăng nhập | `src/login.rs`, `src/qr.rs`, `src/param_cipher.rs`, `src/crypto.rs` | ParamCipher v2, sinh số ngẫu nhiên bằng `getrandom` |
| Tin nhắn / API | `src/chat.rs` (lớn, khoảng 4,6k dòng) | `get_friends`, `get_groups` (`/api/group/getmg-v2`), `get_group_history`, upload ảnh/tệp, `parse_link_metadata(s, url, allow_direct_fetch)` |
| WebSocket | `src/socket.rs` | chỉ dùng `wss://` của Zalo; giải nén giới hạn 32 MiB; `spawn_with_cursors` kéo queue từ con trỏ đã lưu (`WsCursors`) |
| Đồng bộ (0.1.2) | `src/sync_state.rs` | `SyncState` (`sync_state_<uin>.json`), `merge_ws_queue_status`, `gap_signals`/`record_gaps_from` (bảng `missing_ranges`), `MsgIdClock` + `load_conversation_activity` (bảng `thread_activity`) |
| Trace (0.1.2) | `src/trace_shape.rs` | `ZCALL_WS_TRACE=1`: in cấu trúc payload, không in nội dung |
| Video (0.1.2) | `src/video.rs`, `src/gui/media_view.rs` | tìm `ffmpeg.exe`, tham số đã khoá chặt, cửa sổ phát video; `WorkerCmd::PrepareVideo` tải vào `stickers_cache/videos/` |
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

## 5. Việc tiếp theo — xem `docs/dev/KE_HOACH_TIEP_THEO.md`

Người dùng đã chọn hướng (2026-11-01): **v0.1.2 = tự bắt kịp tin nhắn khi mở app** (lưu con trỏ hàng đợi, kéo bù tới khi hết, báo khoảng thiếu, nối liền lịch sử nhóm/Cloud, nhận diện tin E2EE), sau đó **nghiên cứu "Đồng bộ từ điện thoại" theo hướng Sync V1**. Phiên tiếp theo chạy **trên PC Windows** của người dùng, có tài khoản Zalo thật.

- Kế hoạch chi tiết, tiêu chí nghiệm thu, các bước nghiên cứu Sync V1 và cách dựng môi trường PC: `docs/dev/KE_HOACH_TIEP_THEO.md`.
- Prompt dán vào phiên mới: `docs/dev/PROMPT_BAN_GIAO.md`.
- Cơ sở nghiên cứu: `re-notes/09_dong_bo_tin_nhan.md` (bundle Zalo PC trong lịch sử git + tài liệu phân tích Zalo PC 26.10.20 do người dùng cung cấp, tài liệu này không có trong repo).

## 6. Quy trình phát hành (đã chạy thành công cho v0.1.0 và v0.1.1)

1. Sửa `Cargo.toml` thành `version = "<phiên bản mới>"`, chạy `cargo build` để `Cargo.lock` cập nhật dòng `ctdozlite`.
2. `CHANGELOG.md`: thêm mục `## [x.y.z] - <ngày>` (Thêm mới / Sửa lỗi / Bảo mật) và link ở cuối file. Cập nhật `docs/SECURITY_AUDIT.md` nếu có thay đổi bảo mật.
3. `.github/release-notes.md`: cập nhật mục "Điểm mới" cho phiên bản mới. Template có sẵn `{{VERSION}}` / `{{COMMIT}}`.
4. Commit theo Conventional Commits, kết thúc bằng hai dòng attribution mà hệ thống cung cấp cho phiên. Push nhánh làm việc; CI chạy cả bản Windows trên mọi push vào `claude/**`.
5. Khi CI xanh: fast-forward `main` (`git push origin HEAD:main`; trước đó kiểm tra `git merge-base --is-ancestor origin/main HEAD`).
6. Kích hoạt phát hành bằng GitHub MCP:
   - `actions_run_trigger` với `method=run_workflow`, `workflow_id=ci-release.yml`, `ref=main`, `inputs={"release": true}`.
   - Job `release` tự tạo tag `v<version>` tại commit đã build và chỉ chạy khi `test` + `windows` xanh.
   - Release đã tồn tại thì job dừng có chủ ý, nên mỗi lần phát hành phải tăng phiên bản.
7. Kiểm tra bằng `get_release_by_tag v<phiên bản>` rằng release có đủ file:
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
- **(0.1.1) Worktree của agent được tạo từ `73849bd` (commit upstream cũ), không phải `main`.** Cách nhắn agent `git merge --ff-only <commit nền>` vẫn hiệu quả; ghi luôn lệnh này vào đầu prompt giao việc.
- **(0.1.1) Dùng chung `CARGO_TARGET_DIR` giữa các worktree tiết kiệm đĩa nhưng có thể khiến binary là bản build của agent khác** (cùng metadata package). Agent phải `touch` file của mình trước khi build để chụp ảnh và copy binary ngay sau khi build. Đặt tên file chụp/script riêng cho từng agent (scratchpad dùng chung).
- **(0.1.2, PC Windows)** Một số lưu ý khi làm trên máy người dùng:
  - Nút X của QTzalo chỉ **thu nhỏ** cửa sổ; muốn thoát hẳn phải bấm Ctrl+Q. Khi đo "tắt máy", phải kiểm tra tiến trình đã thoát thật.
  - Để không phải nhờ người dùng thao tác, chạy app có hẹn giờ: `timeout 180 ./bin/zalo-rust-xxx.exe --gui --dev` trong Git Bash.
  - Chạy **bản sao** của file exe ở `%USERPROFILE%\qtzalo-trace\bin\`, để `target` vẫn build tiếp được trong lúc app đang chạy.
  - Không chạy hai bản QTzalo cùng lúc với cùng một tài khoản: cả hai cùng ghi `sync_state` và DB.
- **(0.1.2)** `Get-Content`/`Set-Content` của PowerShell 5.1 đọc/ghi theo ANSI và thêm BOM, đã làm hỏng `Cargo.toml` một lần. Sửa file bằng công cụ Edit, không dùng `-replace` + `Set-Content`.
- **(0.1.2)** Git trên PC chưa cấu hình tên/email: commit bằng `git -c user.name=Claude -c user.email=noreply@anthropic.com commit …`. Message nhiều dòng thì truyền qua file (`-F`).
- **(0.1.2)** Scratchpad dùng chung với agent: agent đã ghi đè script chụp ảnh của coordinator. Đặt tiền tố riêng cho từng agent (`a8_…`).
- **(0.1.2)** Bỏ một giới hạn an toàn có thể làm lộ ra lỗi có sẵn. Bỏ giới hạn 3 đợt long-poll đã biến lỗi "không theo dõi queue 16" thành khoảng 10 yêu cầu/giây. Luôn có điều kiện "con trỏ phải tiến" khi lặp, và đếm số yêu cầu trong trace sau mỗi thay đổi vòng lặp.
- **(0.1.2)** Agent bị dừng khi tiến trình cha thoát vẫn giữ nguyên worktree và transcript: `SendMessage` cho nó làm tiếp được, kể cả gộp nhánh coordinator vào.
- **(0.1.1) Mẫu chia việc hiệu quả:** coordinator tự viết toàn bộ backend + biến thể enum + xử lý sự kiện trong `gui.rs`, agent chỉ làm UI theo vùng file (messages/modals-viewer vs modals-settings vs chat/deps). Chỉ có 1 xung đột nhỏ ở khối biến demo trong `src/gui.rs`.

## 8. Tồn đọng đã biết

- **Chưa thử trên Windows thật và tài khoản Zalo thật** cho các tính năng 0.1.1 (và các sửa lỗi lõi #21/#22 của 0.1.0).
- **Chỉ báo đang nhập** chỉ có ở demo. Từ 0.1.2 đã có bằng chứng giao thức (socket `602`, `lp_control` `act = "typing"`), chưa làm.
- ~~Không tự bắt kịp tin offline~~ → đã làm trong 0.1.2. Còn lại: TUI/CLI chưa dùng con trỏ bền; chưa ACK hàng đợi; lịch sử trước lần đăng nhập đầu tiên chỉ lấy được qua "đồng bộ từ điện thoại" (Phần B). Danh sách đầy đủ ở `KE_HOACH_TIEP_THEO.md` §4.
- **Số thành viên nhóm** dựa trên `totalMember` suy ra từ entity `Group` của web bundle cũ (đã xoá ở `54314e0`); chưa thấy trong payload thật. Thiếu thì header vẫn hiện "Nhóm".
- **S6:** media cuộc gọi chưa lọc nguồn gói UDP; client khai `srtpMode: 0`.
- **#19:** chưa đồng bộ toàn bộ lịch sử.
- **Cảnh báo `cargo audit`:** `audiopus_sys`, `ttf-parser` hết bảo trì (không có lỗ hổng).
- **Tải về:** chưa có nút huỷ; macOS chưa gắn quarantine; dark theme nút đỏ (`pal.danger`) chữ trắng tương phản hơi thấp.
- **Emoji màu:** egui không vẽ được emoji màu.
- **Bộ cài chưa ký số**, nên SmartScreen cảnh báo.
