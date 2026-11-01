# Kế hoạch công việc tiếp theo — v0.1.2 và nghiên cứu Sync V1

> Viết ngày 2026-11-01, sau khi phát hành v0.1.1 (`main` = `d8ba11c`, nghiên cứu ở `adc597f`).
> Đọc kèm: `docs/dev/BAN_GIAO.md` (trạng thái, quy trình, bài học) và `re-notes/09_dong_bo_tin_nhan.md` (cơ chế đồng bộ của Zalo PC, có nhãn [O]/[I]/[U]).
> Phiên tiếp theo chạy **trên PC Windows của người dùng**: có Zalo PC thật, tài khoản Zalo thật, điện thoại thật. Xem §3 để chuẩn bị môi trường.

## 0. Mục tiêu do người dùng nêu

> "Mở PC sau 1 ngày không dùng, Zalo PC phải tự đồng bộ ngay những tin nhắn đã có trên điện thoại. Cài trên PC mới thì đồng bộ được tin cũ."

Có ba việc tương ứng (chi tiết: `re-notes/09` §1):

| Việc | Cơ chế Zalo | Phiên bản |
|---|---|---|
| Bắt kịp tin sau thời gian tắt máy | Hàng đợi offline phía máy chủ + con trỏ lưu bền | **v0.1.2 (Phần A)** |
| Nhóm / Cloud đủ lịch sử | `/api/cm/getrecentv2`, `getoldv2`, `rgetv2` | **v0.1.2 (Phần A)** |
| Máy mới lấy tin 1-1 cũ | "Đồng bộ từ điện thoại" — sync v1 (file backup) / sync v2 (WASM) | **Nghiên cứu (Phần B)**, chỉ code sau khi có bằng chứng |

Thứ tự bắt buộc: **Phần A trước** (làm được ngay, rủi ro thấp, giải quyết trường hợp "1 ngày"), phát hành v0.1.2, nhờ người dùng thử với tài khoản thật. **Phần B sau.**

---

## 1. Phần A — v0.1.2: tự bắt kịp tin nhắn khi mở app

### A0. Thu bằng chứng trước khi sửa (trên PC, tài khoản thật) — BẮT BUỘC

Mục đích: xác nhận các giả thiết **[I]/[U]** trong `re-notes/09` trước khi viết code.

1. Build bản hiện tại (§3.2), chạy có trace:
   ```powershell
   $env:ZCALL_WS_TRACE = "1"
   .\target\debug\zalo-rust.exe --gui --dev 2> "$env:USERPROFILE\qtzalo-trace\ws-$(Get-Date -f yyyyMMdd-HHmm).log"
   ```
   Trace in **nội dung tin nhắn đã giải mã**. Lưu ngoài repo (`%USERPROFILE%\qtzalo-trace\`), **không commit**, không dán nguyên văn lên GitHub.
2. Kịch bản: đóng QTzalo → nhận khoảng 20 tin 1-1 và 20 tin nhóm trên điện thoại, tự gửi vài tin từ điện thoại → mở lại QTzalo có trace.
3. Ghi lại vào `re-notes/10_bang_chung_dong_bo.md`. Chỉ ghi **cấu trúc** (tên trường, kiểu, số lượng), che ID và nội dung:
   - Danh sách queue trong `AUTHEN` (`data.qCmds`, `data.ctrl_qCmds`) — có `510_0/511_0/510_1/511_1/513_0/713_0` không?
   - Phản hồi kéo queue với `first:true, lastId:1`: máy chủ trả gì? Có `more`, `queueStatus[q].lastId`, `ids`, `evict`, `isLastQueue`, `resetLastActionId` không? Có trả lại các tin trong lúc tắt máy không?
   - Long-poll `getChatFromPolling` với cursor `"1"` trả gì; thử thêm queue `"6"` (nhóm offline theo `queue_actionid_map`) **bằng một biến môi trường thử nghiệm**, không bật mặc định.
   - Có tin E2EE không (queue `513`/`713`, trường `e2eeStatus`, nội dung là bản mã)?
4. Kết luận cho từng giả thiết: **xác nhận / bác bỏ / chưa rõ**. Thiết kế A1–A5 phải theo kết luận này.

### Thiết kế đã chốt sau A0 (2026-11-01)

Bằng chứng: `re-notes/10_bang_chung_dong_bo.md` (3 lượt chạy với tài khoản thật). Thay đổi so với A1–A5 bên dưới:

| Mục | Quyết định |
|---|---|
| A1 | Giữ. Queue thật là `510_1, 511_1, 515_0, 517_0, 518_0` + ctrl `603_1, 610_1, 611_1`; long-poll `1, 15, 16`. Kéo về không xoá tin khỏi hàng đợi, nên `lastId: 1` tải lại mọi thứ từ lúc đăng nhập. Con trỏ do `socket.rs` tự cập nhật trong một bản đồ dùng chung; worker lưu xuống `sync_state_<uin>.json`. |
| A2 | Giữ, và thêm: `RefreshThreads` phải thử lại khi `check_session` trả `Unreachable` (lượt 2 bị kẹt và không thử lại). |
| A3 | Giữ. `evict` có thật trong `queueStatus` của mọi queue; chưa gặp giá trị khác 0. |
| A4 | Thu nhỏ: thiết bị mới nhận 0 tin nhóm từ `getrecentv2` (cờ `isFiltered`). Chỉ nối lịch sử cho "Cloud của tôi" và nhóm nào có trả tin. |
| A5 | **Bỏ** khỏi v0.1.2: không thấy tin E2EE nào; queue E2EE của phiên là `515/517` (web). |
| A8 (mới) | Tin ảnh/video/tệp đến từ queue đang hiện thành link thô và mở trình duyệt. Sửa parse để hiện thẻ ảnh/video/tệp; bấm ảnh → cửa sổ xem ảnh riêng của app; bấm video → cửa sổ phát video của app (ffmpeg nếu có, không có thì tải vào cache rồi mở bằng trình phát mặc định); không mở trình duyệt. Người dùng chọn phương án này. |
| A9 (mới) | Danh sách hội thoại giống điện thoại: tab Tin nhắn chỉ hiện hội thoại có hoạt động, sắp theo thời điểm hoạt động cuối. Nguồn thứ tự: tin trong DB + `clearUnreads[].lastMsgId` của `get-last-msgs` (137 hội thoại). `msgId` tăng gần tuyến tính theo thời gian (khoảng 41 id/ms, 1 đảo thứ tự trong 75 tin) nên đổi được ra thời điểm ước lượng. Người dùng chọn "chỉ hội thoại có hoạt động". |

Phân công: coordinator làm A1–A4, A9 và các điểm nối (`GuiEvent::{Video*, SyncProgress}`, `WorkerCmd::PrepareVideo`, `paths::video_cache_dir`, khung `src/gui/media_view.rs`); một agent làm A8 trong worktree riêng.

### A1. Lưu con trỏ hàng đợi bền theo tài khoản

- File mới `src/sync_state.rs` (module thuần + I/O nhỏ, có unit test):
  - `struct SyncState { version: u32, uin: String, poll: chat::PollState, ws_queues: BTreeMap<String /*"510_0"*/, QueueCursor { last_id: String, pre_ids: Vec<String> }>, updated_at_ms: i64 }`
  - `load(uin) -> SyncState` (thiếu hoặc hỏng file thì dùng mặc định, ghi log), `save(&self)` dùng `paths::write_private` (ghi nguyên tử, quyền 0600).
  - Đường dẫn: `paths::data_dir().join(format!("sync_state_{uin}.json"))`. Thêm vào `paths.rs` một hàm `sync_state_file(uin)`. Làm sạch `uin` (chỉ cho chữ số) để không có path traversal.
- `src/gui/worker.rs`: thay `PollState::default()` (khoảng `:350`, `:538`, `:559`, `:1989`) bằng `SyncState::load(uin)`. Chỉ reset khi **đổi tài khoản** hoặc **đăng xuất**, không reset khi mất mạng. Lưu sau mỗi đợt kéo thành công, có debounce khoảng 2 giây.
- `src/socket.rs`: `pull_frame` nhận `lastId`/`preIds` đã lưu thay vì luôn `1`. Mỗi lần nhận `queueStatus`/`lastActionId` thì gửi cập nhật con trỏ ra ngoài (thêm trường vào `WsEvent` hoặc một kênh riêng) để worker lưu.
- `src/main.rs` (đường TUI/CLI) và `src/tui.rs`: dùng cùng cơ chế nạp/lưu.
- Test: lưu → nạp lại giống hệt; file hỏng → mặc định; `uin` chứa `../` → bị từ chối; không lẫn dữ liệu giữa hai tài khoản.

### A2. Kéo bù tới khi hết, có tiến trình

- `chat::poll_new_messages` (`src/chat.rs:~3150`): bỏ giới hạn `for _ in 0..3`; lặp tới khi `more == 0` (hoặc `isLastQueue`), trần an toàn 200 đợt, có thể huỷ.
- Socket: đã tự `pull next` khi `more == 1` (`src/socket.rs:~550`); bảo đảm dùng đúng `lastId` của từng queue.
- GUI: sự kiện mới `GuiEvent::SyncProgress { fetched: usize, done: bool }`. Hiện một dải mảnh dưới header danh sách hội thoại ("Đang đồng bộ tin nhắn… 37 tin"), giữ đúng phong cách theme hiện tại. Có biến demo `QTZALO_DEMO_SYNC=1` để chụp ảnh.
- Ghi DB như hiện nay (`store::persist_threads`). Chống trùng theo `msg_id` (đã có), kiểm tra lại với tin gửi từ điện thoại (`cliMsgId`).

### A3. Phát hiện khoảng thiếu và báo cho người dùng

- Khi `resetLastActionId` khác 0, hoặc `queueStatus[q].evict == true` (theo tài liệu Zalo PC 26.10.20): ghi một khoảng thiếu.
- `src/store.rs`: bảng mới `missing_ranges(id INTEGER PRIMARY KEY, conv_id TEXT /*"*" = mọi hội thoại*/, from_ts INTEGER, to_ts INTEGER, reason TEXT, created_at INTEGER)`, có migration và truy vấn tham số hoá.
- GUI: banner giống Zalo — "Tin nhắn chưa được đồng bộ. Có thể thiếu tin từ dd/mm hh:mm đến dd/mm hh:mm. Xem đầy đủ trên điện thoại." — có nút đóng; đóng thì đánh dấu đã xem.
- Không tự gọi bất kỳ API "đồng bộ từ điện thoại" nào.

### A4. Nhóm và Cloud: nối liền lịch sử

- Sau `getrecentv2` (50 tin): nếu tin cũ nhất trả về vẫn **mới hơn** tin mới nhất đã có trong DB của hội thoại đó thì gọi tiếp `getoldv2` với `globalMsgId` của tin cũ nhất, cho tới khi nối liền. Trần 10 trang mỗi nhóm mỗi lần khởi động; chạy nền, ưu tiên hội thoại đang mở.
- Hàm thuần `needs_gap_fill(local_newest_ts, page_oldest_ts) -> bool` có unit test.
- Nút "Tải thêm tin nhắn cũ" giữ nguyên.

### A5. Nhận diện tin E2EE

- Trong `merge_previews` / `parse_message`: nếu tin đến từ queue `513/514/515/713/715` hoặc có `e2eeStatus` khác 0 hay nội dung là bản mã, hiện một bong bóng giữ chỗ "🔒 Tin nhắn mã hoá đầu cuối — xem trên điện thoại" (văn bản thường, không phụ thuộc emoji màu). Không hiện rác.
- Chỉ làm khi A0 cho thấy có tin E2EE thật; cấu trúc tin E2EE lấy từ trace.

### A6. Không làm trong v0.1.2

- ACK hàng đợi (`DELETE_QUEUE_ACK_*`): ACK sai làm mất tin vĩnh viễn. Chỉ ghi vào tồn đọng kèm bằng chứng từ A0.
- `VERIFY_MSG 532` / `GET_MSG 570` để tải bù tin lẻ: chỉ làm nếu A0 thấy `534 PUSH_MISS_MSG` thật.
- Giải mã E2EE (Signal), đồng bộ từ điện thoại.

### A7. Nghiệm thu v0.1.2

- `cargo test --locked` xanh. `bash scripts/check-windows.sh` (trên Linux/CI) hoặc build MSVC trên PC.
- **Kịch bản thật:** lặp lại A0 bước 2 với thời gian tắt 1 giờ, 1 ngày (nếu kịp) → mọi tin hiện đủ, đúng thứ tự, không trùng; trace cho thấy kéo tới `more = 0`; con trỏ được lưu (`sync_state_<uin>.json` thay đổi sau mỗi lần chạy).
- Ảnh chụp `--demo` cho dải tiến trình và banner thiếu tin (sáng và tối).
- Tài liệu: CHANGELOG `[0.1.2]`, `.github/release-notes.md`, `SECURITY_AUDIT` (file `sync_state` chứa con trỏ, không chứa khoá → ghi quyền 0600), `BAN_GIAO.md`.
- Phát hành theo `BAN_GIAO.md` §6 (người dùng đã cho phép merge fast-forward vào `main` và phát hành từ `main` ở các bản trước; **hỏi lại người dùng một câu xác nhận trước khi phát hành v0.1.2**).

---

## 2. Phần B — Nghiên cứu "Đồng bộ từ điện thoại" theo hướng Sync V1

### B0. Vì sao chọn Sync V1

Theo tài liệu phân tích Zalo PC 26.10.20 (người dùng cung cấp; tóm tắt ở `re-notes/09` §8):
- **Sync v2** (WS `590/591/592`, mã hoá Sync2 trong WASM `libzproto_wasm`: Ed25519 + ChaCha20 + phiên kiểu Signal + zstd/protobuf). AEAD/KDF chưa xác định → quá khó ở giai đoạn này.
- **Sync v1 / transfer**: đường HTTP rõ ràng, khoá RSA-2048 chuẩn. Ẩn số chính chỉ là **định dạng file backup** do module native `db-cross-v4` xử lý. → khả thi nhất.

### B1. Luồng Sync V1 đã biết (theo tài liệu, chưa kiểm chứng trên QTzalo)

```
PC: sinh RSA-2048 (public key SPKI/DER → base64), TTL = cross_setting.timeoutKey (giờ)
PC → HTTP GET /api/message/pull_mobile_msg   (cmd 12000)
      params (encodeAES như các API khác):
      { pc_name, public_key, from_seq_id = maxSeq + 1, min_seq_id = minSeq,
        is_retry, temp_key, imei }
Điện thoại: hiện yêu cầu → người dùng xác nhận → xuất DB tin nhắn, nén + mã hoá, tải lên
PC ← control event (kênh ctrl của socket — cmd/queue CHƯA XÁC ĐỊNH, xem B3):
      USER_CONFIRM / USER_REJECT / MOBILE_IDLE / MOBILE_ACTIVE / MOBILE_RESTORING
      BACKUP_SUCCESS { db_info: { backup_db:{msg_thread,msg_total}, origin_db:{…}, db_format },
                       file_name, url, checksum_code|checksum, encrypted_key, is_full_transfer, … }
      BACKUP_FAIL
PC: tải file (url, kiểm checksum) → <dir>/blob
PC: privateKey = RSA_PKCS1_v1_5_decrypt(encrypted_key)   (format 0 = base64, 1 = hex)
PC: db-cross-v4.decompressAndDecryptDb(input, output, password)   ← ĐỊNH DẠNG CHƯA BIẾT
PC: RESTORE_CONVERSATIONS / RESTORE_MESSAGES với { plainUserId: rawUid, noiseUserId: userId,
     password: uin, format, conversations, checkpoint }
PC: lưu minSeq / maxSeq (con trỏ v1) cho lần sau
Huỷ: GET /api/message/cancel_pull_mobile_msg (12700) {pc_name, public_key, imei}
Dọn: GET /api/message/delete_snapshot_mobile_msg (12003) {public_key, imei}
```

### B2. Chuẩn bị (trên PC)

- **Nguyên tắc pháp lý/bảo mật:**
  - Chỉ phân tích để tương thích, trên tài khoản của chính người dùng.
  - **Không** commit mã/JS/WASM/native của Zalo vào repo (bài học `54314e0`).
  - Để mọi file trích xuất, trace, backup ngoài repo, ví dụ `%USERPROFILE%\zalo-re\`.
  - Nếu có thể, thử trước trên tài khoản phụ.
- Thư mục cài: `%LOCALAPPDATA%\Programs\Zalo\Zalo-<phiên bản>\resources\` (tài liệu ghi `Zalo-26.10.20`). Ghi lại phiên bản thật.
- Công cụ: Node.js LTS, `npx @electron/asar` (giải nén `app.asar`), 7-Zip, `dumpbin` (đi kèm VS Build Tools) hoặc `rabin2`, `strings` (Sysinternals). Tuỳ chọn: Ghidra (phân tích `.node`).

### B3. Các bước nghiên cứu

1. **Giải nén và lập bản đồ** (không chạy gì):
   - `npx @electron/asar extract app.asar %USERPROFILE%\zalo-re\app`.
   - Tìm các module native: `Get-ChildItem -Recurse -Filter *.node` — tìm `db-cross-v4`, sqlite, sqlcipher.
   - Grep các chuỗi: `pull_mobile_msg`, `BACKUP_SUCCESS`, `encrypted_key`, `decompressAndDecryptDb`, `DECRYPT_BACKUP`, `RESTORE_CONVERSATIONS`, `RESTORE_MESSAGES`, `db_format`, `temp_key`, `sync_cross_state`, `cancel_pull_mobile_msg`.
   - Ghi vào `re-notes/11_sync_v1.md`: file nào, offset nào, đoạn mã ngắn đã làm gọn (đủ để hiểu, không chép cả khối).
2. **Xác định kênh nhận `BACKUP_SUCCESS`**: control event đi qua cmd nào (`601`/`602`/`603`?), trường `act`/`act_type` gì. QTzalo đã có `call::extract_voip_controls` lọc `act_type == "voip"` → cần một bộ lọc tương tự cho sự kiện transfer.
3. **Định dạng `db-cross-v4`:**
   - `dumpbin /exports <file>.node` + `strings` để tìm tên hàm, thư viện nén (zstd/lz4/zlib/brotli), thuật toán (AES-CBC/GCM, ChaCha, PBKDF2/SHA), dấu hiệu SQLCipher.
   - **Dùng module làm "hộp đen" (oracle):** chạy chính `.node` bằng Node cùng ABI với Electron của Zalo. Thử `ELECTRON_RUN_AS_NODE=1 Zalo.exe script.js`; fuse `RunAsNode` có thể đã tắt, khi đó tải đúng phiên bản Electron. Đưa vào một file backup thật (từ B4) để ra DB giải mã, rồi xem schema bằng `sqlite3`.
   - Mục tiêu: mô tả định dạng đủ để **viết lại bằng Rust**. Nếu không viết lại được thì ghi rõ và dừng; **không** đóng gói hay gọi module native của Zalo trong QTzalo.
4. **Schema DB của điện thoại sau giải mã**: bảng hội thoại/tin nhắn, cột thời gian, `msgId`/`globalMsgId`/`cliMsgId`, cách mã hoá cột (`message` có thể bị mã hoá thêm — tài liệu nhắc `BasicEncryption`/`AESCBCEncryption`), `noise id ↔ plain id`.

### B4. Probe thử nghiệm trong QTzalo (chỉ chế độ `--dev`, sau B3 bước 1–2)

- Lệnh dev mới, ví dụ `zalo-rust --dev --sync-v1-probe`, ở file riêng `src/bin/sync_v1_probe.rs` (giống `call_probe`), **không** đưa vào GUI:
  1. Nạp `session.json` thật.
  2. Sinh RSA-2048 bằng crate `rsa`. Kiểm tra MSRV ≤ 1.88 và `cargo audit`: crate `rsa` có RUSTSEC-2023-0071 (Marvin, rò rỉ thời gian khi giải mã). Ghi vào SECURITY_AUDIT, hoặc cân nhắc `aws-lc-rs`.
  3. Gọi `pull_mobile_msg` với `from_seq_id`/`min_seq_id` lấy từ B3 (lần đầu có thể là full transfer).
  4. Mở socket có trace, chờ `USER_CONFIRM` → `BACKUP_SUCCESS` (người dùng xác nhận trên điện thoại).
  5. Tải file, kiểm checksum, giải `encrypted_key` bằng RSA PKCS#1 v1.5, lưu file và khoá **ngoài repo**.
  6. Có huỷ (`cancel_pull_mobile_msg`) và dọn (`delete_snapshot_mobile_msg`) khi thoát.
- Mọi bước ghi trace ra file ngoài repo; in ra màn hình chỉ cấu trúc.

### B5. Tích hợp (chỉ khi B3 + B4 thành công và người dùng đồng ý)

- `src/sync_v1/` (thuần Rust): `rsa_session`, `download`, `decrypt` (định dạng đã viết lại), `import` (ánh xạ schema điện thoại → `store.rs`, `src = SYNC_MOBILE_DB`, chống trùng theo `msg_id`, có checkpoint `minSeq/maxSeq/currentSeq`, tiếp tục được khi bị ngắt).
- GUI: Cài đặt → "Đồng bộ tin nhắn từ điện thoại" theo đúng các bước của Zalo: xác nhận → chờ điện thoại → tải → giải mã → khôi phục, có tiến trình (trọng số 10/20/70), huỷ được, thông báo lỗi theo bảng mã lỗi.
- Bảo mật: khoá RSA chỉ giữ trong RAM; file tạm trong thư mục dữ liệu, quyền 0600, xoá sau khi khôi phục; nhập dữ liệu bằng truy vấn tham số hoá; giới hạn dung lượng file; không ghi nội dung tin vào log.

### B6. Câu hỏi phải trả lời trước khi viết code tích hợp

1. Endpoint `pull_mobile_msg` có chấp nhận client QTzalo không (UA, `imei`, `zpw_type=30`)? Lỗi gì nếu không?
2. `BACKUP_SUCCESS` đến qua kênh nào, cấu trúc chính xác ra sao?
3. Định dạng `db-cross-v4` có viết lại được bằng Rust không (nén gì, mã hoá gì, khoá từ đâu: `privateKey` sau RSA, hay `uin`)?
4. Có cần `temp_key` hay liên kết thiết bị tin cậy không?
5. Có rủi ro khoá tài khoản không? Quan sát sau probe: thông báo bảo mật trên điện thoại, đăng xuất bất thường.

---

## 3. Môi trường PC (Windows)

### 3.1 Cài đặt một lần

- Git, **Rust stable** (rustup, toolchain MSVC), **Visual Studio 2022 Build Tools** với workload "Desktop development with C++" (cung cấp MSVC, Windows SDK và `dumpbin`), **CMake** (bản đi kèm VS là đủ).
- Tuỳ chọn: Node.js LTS (Phần B), `ffmpeg` trong `PATH` (gọi video), Inno Setup (chỉ khi muốn tự build bộ cài; CI đã làm việc này).

### 3.2 Build, test, chạy

```powershell
git clone https://github.com/quangthoai1985/QTzalo.git; cd QTzalo
git switch -c claude/v0.1.2            # CI chạy trên mọi push vào main và claude/**
$env:CMAKE_POLICY_VERSION_MINIMUM = "3.5"
cargo build --locked --bin zalo-rust
cargo test --locked
.\target\debug\zalo-rust.exe --gui --demo     # giao diện mẫu, không mạng, không ghi dữ liệu
.\target\debug\zalo-rust.exe --gui            # tài khoản thật, dữ liệu ở %APPDATA%\QTzalo
```

- Nên sao lưu `%APPDATA%\QTzalo` trước khi thử bản dev với tài khoản thật. Hoặc đặt `$env:QTZALO_DATA_DIR` sang một thư mục thử và đăng nhập QR lại.
- App là ứng dụng console nên chuyển hướng được stderr (`2> file.log`). Trace socket: `$env:ZCALL_WS_TRACE="1"`; log lỗi chỉ ghi khi `--dev`.
- Phát hành: dùng `gh` nếu đã đăng nhập (`gh workflow run ci-release.yml --ref main -f release=true`), hoặc nút *Run workflow* trên GitHub (tick `release`). Các bước khác theo `BAN_GIAO.md` §6.

### 3.3 Khác biệt so với phiên cloud trước

- Có mạng thật tới Zalo: kiểm chứng được với tài khoản thật. Nhưng mọi lệnh gọi tới máy chủ Zalo đều là **hành động thật trên tài khoản của người dùng** → hỏi trước khi chạy probe (Phần B4) hoặc bất cứ thứ gì gửi tin, xác nhận hay xoá.
- Người dùng phải tự quét QR và xác nhận trên điện thoại; agent không thao tác được điện thoại.
- Không chụp ảnh Xvfb được: dùng `--demo` rồi chụp bằng `Win+Shift+S`, nhờ người dùng gửi ảnh, hoặc chạy `scripts/check-windows.sh` trong WSL.

---

## 4. Tồn đọng khác (sau Phần A/B, khi người dùng yêu cầu)

| Việc | Ghi chú |
|---|---|
| Kiểm chứng tính năng 0.1.1 trên Windows thật | MotW (`Get-Content <file> -Stream Zone.Identifier`), "Mở thư mục" (`explorer /select`), hộp chọn thư mục, xoá cache, số thành viên nhóm (`totalMember`) |
| Nút huỷ khi đang tải tệp; macOS gắn `com.apple.quarantine` | `src/download.rs`, `src/gui/messages.rs` |
| Lưu `member_count` vào `Thread` | Cần sửa struct literal ở `demo.rs`, `modals.rs`, `worker.rs`, `src/bin/test_fetch.rs` |
| Chỉ báo "đang nhập" thật | Cần trace frame 601/602 lúc đối phương gõ |
| ACK hàng đợi offline | Sau khi A0 có bằng chứng |
| Giải mã E2EE (Signal) | Việc lớn; cần đăng ký thiết bị E2EE + trusted device |
| S6 lọc UDP cuộc gọi, ký số bộ cài, đổi tên binary | Cần quyết định của người dùng |
