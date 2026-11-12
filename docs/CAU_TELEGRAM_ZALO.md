# Cầu Telegram ↔ Zalo (`--tg-bridge`)

Chat với người dùng Zalo đầu kia hoàn toàn từ Telegram. Telegram giữ file/ảnh
vĩnh viễn; đĩa local chỉ là bộ đệm chuyển tiếp (quota LRU).

## Chạy

```bash
export TELEGRAM_BOT_TOKEN="<token bot>"
zlite --tg-bridge \
  --tg-map "-100xxx/2:<zalo_thread_id>,<chat_id_2>:<zalo_tid_2>" \
  --tg-owner <telegram_user_id> \
  [--bridge-quota-gb 10] [--dry-run]
```

- `--tg-map`: cặp `tg_chat[/topic]:zalo_thread_id`, phân cách dấu phẩy.
  Mỗi hội thoại Zalo = 1 forum topic Telegram (học từ `zalo-tg`).
- `--tg-owner`: chỉ user ID này mới điều khiển được bridge (mặc định 0 = ai cũng được, chỉ dùng test local).
- `--dry-run`: log 2 chiều, không gửi gì.
- `--bridge-quota-gb`: quota bộ đệm `data_dir/bridge/` (mặc định 10, min 1).

## Luồng

```
Zalo poll (có sẵn) ──▶ bridge inbox ──▶ Telegram sendMessage/Photo/Document
Telegram long-poll ──▶ bridge outbox ──▶ Zalo send_message / send_attachment (chunk 1MB + resume)
SQLite bridge_files: msg_id ↔ path ↔ tg_file_id ↔ zalo_url ↔ offset ↔ last_access
```

- Echo-guard: bảng riêng `bridge_sent(tg_key)` — tin bridge gửi đi không bao giờ forward ngược.
- Reply-chain: Zalo→TG giữ `reply_parameters`; TG→Zalo tra mapping ngược (best-effort, thiếu thì gửi thường).
- Album: gom media gửi liền nhau 500ms thành 1 album (học từ `zalo-tg`), tránh spam.
- File đơn > quota trống → từ chối ngay + báo rõ, không tải nửa vời.
- Xả LRU: vượt 90% quota → xóa file cũ nhất (trừ file đang upload dở) tới khi còn 70%. Mapping DB giữ lại vĩnh viễn để tái dùng `tg_file_id`/URL.

## Giới hạn PR1 (text)

- PR1: text 2 chiều + mapping + echo-guard + quota + CLI `--dry-run`.
- PR2: media (album buffer + resume + local Bot API doc).
- PR3: recall 2 chiều + reply-chain đầy đủ.

## Auth

Token qua env `TELEGRAM_BOT_TOKEN` (12-factor). Keychain (`zlite --auth`) dùng cho LLM; Telegram dùng env riêng để tách phạm vi.
