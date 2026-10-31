# Đặc tả giao thức Zalo Web/PC

Tài liệu tổng hợp cấu trúc tham số, bảng phân bổ domain và các thuật toán mã hóa sử dụng trong `ctdozlite`.

---

## 1. Tham số cơ sở

- `zpw_ver`: `694`
- `zpw_type`: `30` (PC / Web client)
- Query string chung trên các request: `zpw_ver=694&zpw_type=30`

---

## 2. Bảng phân bổ máy chủ (Service Domains)

| Dịch vụ | Host mặc định | Endpoint chính |
|---|---|---|
| **Auth** | `https://wpa.zaloapp.com` | `/api/login/reqqr`, `/api/login/loginByPasswv2` |
| **WPA Session** | `https://wpa.chat.zalo.me` | `/api/login/getServerInfo`, `/api/login/getLoginInfo` |
| **Profile & Danh bạ** | `https://tt-profile-wpa.zaloapp.com` | `/api/social/friend/getfriends`, `/api/social/profile/getminiprofiles` |
| **Hội thoại** | `https://tt-convers-wpa.api.zaloapp.com` | `/api/message/get-last-msgs`, `/api/pinconvers/list` |
| **Nhóm chat** | `https://tt-group-wpa.zaloapp.com` | `/api/group/getlg/v4`, `/api/group/sendmsg`, `/api/group/history` |
| **Tin nhắn 1-1** | `https://tt-chat3-wpa.api.zaloapp.com` | `/api/message/sms`, `/api/message/sticker`, `/api/message/quote` |
| **Sticker API** | `https://tt-sticker-wpa.chat.zalo.me` | `/api/message/sticker/suggest/keywords`, `/api/message/sticker/suggest/stickers`, `/api/message/sticker/search`, `/api/message/sticker/sticker_detail` |
| **Sticker CDN** | `https://api.zaloapp.com` | `/api/emoticon/sticker/webpc?eid=<id>&size=130` |

---

## 3. Chữ ký `signkey`

Các endpoint khởi tạo và xác thực yêu cầu tham số `signkey` tính bằng MD5:

```text
signkey = md5("zsecure" + tag + concat(params[k] for k in sorted(keys)))
```

- `tag`: tên lệnh viết thường, bỏ dấu gạch ngang (ví dụ `reqqr`, `getlogininfo`, `getserverinfo`, `loginbypasswv2`).
- Các tham số phụ như `ref` hoặc `nretry` được nối vào URL sau khi đã tính `signkey`.

---

## 4. Mã hóa tham số xác thực (`ParamCipher v2`)

Sử dụng cho các gói tin chưa có khóa phiên `zpw_enk`:

1. Khóa tĩnh `PARAM_CIPHER_KEY = "3FC4F0D2AB50057BCE0D90D9187A22B1"`.
2. Sinh `zcid`: mã hóa chuỗi `"{type},{imei},{firstLaunchTime}"` bằng AES-256-CBC với `PARAM_CIPHER_KEY` và IV toàn `0`, sau đó chuyển sang chuỗi hex in hoa.
3. Sinh `zcid_ext`: chuỗi hex ngẫu nhiên dài 10 ký tự.
4. Dẫn xuất khóa `encrypt_key` (32 ký tự ASCII) từ các vị trí chẵn/lẻ của `MD5(zcid_ext)` và `zcid`.
5. Mã hóa JSON payload bằng AES-256-CBC (`encrypt_key`, zero IV, PKCS7) và mã hóa Base64 vào trường `params`.

---

## 5. Khóa phiên `zpw_sek` và `zpw_enk`

- **`zpw_sek` (Session Cookie):** Nhận được sau khi xác nhận QR hoặc đăng nhập thành công. Giá trị này được gắn vào cookie jar (`Cookie: zpw_sek=...; _zlang=vi`) cho toàn bộ các domain dịch vụ của Zalo (`*.zaloapp.com`, `*.chat.zalo.me`) kèm header `Origin: https://chat.zalo.me`.
- **`zpw_enk` (Session Data Key):** Nhận từ `getLoginInfo`, dùng làm khóa AES-128-CBC (zero IV, PKCS7) để mã hóa tham số `params` và giải mã trường `data` trên toàn bộ các API dữ liệu sau đăng nhập.
