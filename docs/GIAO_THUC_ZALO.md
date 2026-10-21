# Đặc Tả Giao Thức Mật Mã Zalo PC (Reverse Engineering Specification)

Tài liệu này ghi lại toàn bộ kết quả phân tích kỹ thuật đảo ngược (Reverse Engineering) từ gói bundle Zalo PC Electron v26.10.11 (`compact-app-pc-*.js`) và dữ liệu mạng trực tiếp (`zalo.flow`).

---

## 1. Hệ thống Tham số Cơ sở

- **`zpw_ver`**: `694` (Phiên bản giao thức web/PC).
- **`zpw_type`**: `30` (Loại client: PC desktop client).
- **Common query string**: `zpw_ver=694&zpw_type=30`.

---

## 2. Bảng Phân Bổ Máy Chủ (Domain Routing Table)

Máy chủ Zalo sử dụng cụm máy chủ phân tán theo chức năng dịch vụ:

| Tên dịch vụ | Host chuẩn | Chức năng |
|---|---|---|
| **Auth Domain** | `https://wpa.zaloapp.com` | Xin mã QR (`reqqr`), đăng nhập mật khẩu (`loginByPasswv2`). |
| **WPA Chat** | `https://wpa.chat.zalo.me` | Khởi tạo thông tin phiên (`getServerInfo`, `getLoginInfo`). |
| **Profile & Friend** | `https://tt-profile-wpa.zaloapp.com` | Lấy danh bạ (`getfriends`), tra cứu người lạ (`getminiprofiles`), trạng thái (`me-v2`). |
| **Conversations API** | `https://tt-convers-wpa.api.zaloapp.com` | Lấy tin nhắn mới nhất (`get-last-msgs`), ghim (`pinconvers`), đánh dấu unread. |
| **Group API** | `https://tt-group-wpa.zaloapp.com` | Danh sách nhóm (`getlg/v4`), gửi tin nhóm (`sendmsg`), lịch sử nhóm (`history`). |
| **Chat API (Send)** | `https://tt-chat3-wpa.api.zaloapp.com` | Gửi tin nhắn cá nhân (`/api/message/sms`). |
| **Polling Host** | `https://p3-msg-chat.zaloapp.com` | Long-polling nhận tin nhắn tức thì. |

---

## 3. Thuật toán Ký Số `SignKey` (Request Signature)

Mọi yêu cầu gửi lên các endpoint xác thực (như `reqqr`, `loginByPasswv2`, `getLoginInfo`) đều bắt buộc mang tham số chữ ký `signkey`:

### Công thức:
$$\text{signkey} = \text{MD5}("zsecure" + \text{tag} + \sum_{k \in \text{sorted(keys)}} \text{params}[k])$$

- **`tag`**: Tên endpoint viết thường, loại bỏ toàn bộ dấu gạch ngang (ví dụ: `/api/login/reqqr` -> `reqqr`, `/api/login/getLoginInfo` -> `getlogininfo`, `/api/login/loginByPasswv2` -> `loginbypasswv2`).
- **Quy tắc quan trọng:**
  - Các tham số phụ như `ref` và `nretry` được thêm vào URL **SAU** khi tính `signkey` và **KHÔNG** tham gia vào chuỗi hash MD5. Nếu đưa vào sẽ dẫn đến lỗi `18022 Invalid Signature`.

---

## 4. Thuật toán Tham số Mã hóa `ParamCipher v2`

Áp dụng cho các yêu cầu xác thực nhạy cảm:

1. **Khóa Master tĩnh:**
   $$\text{PARAM\_CIPHER\_KEY} = \text{"3FC4F0D2AB50057BCE0D90D9187A22B1"}$$
2. **Khóa `zcid`:**
   $$\text{zcid} = \text{AES-256-CBC-Encrypt}(\text{PARAM\_CIPHER\_KEY}, \text{IV}=0, \text{"\{type\},\{imei\},\{firstLaunchTime\}"}).\text{hex}().\text{upper}()$$
3. **Mã ngẫu nhiên `zcid_ext`:** Sinh chuỗi hex ngẫu nhiên độ dài 10 ký tự.
4. **Dẫn xuất khóa mã hóa động `encrypt_key` (32 ký tự ASCII):**
   $$\text{hash} = \text{MD5}(\text{zcid\_ext}).\text{hex}().\text{upper}()$$
   $$\text{encrypt\_key} = \text{even}(\text{hash})[0..8] + \text{even}(\text{zcid})[0..12] + \text{reverse}(\text{odd}(\text{zcid}))[0..12]$$
5. **Mã hóa Payload:** Payload JSON được mã hóa bằng AES-256-CBC (khóa `encrypt_key`, zero IV, PKCS7) -> Encode Base64 -> Đặt vào tham số `params`.

---

## 5. Thuật toán Đăng Nhập Mật Khẩu `PasswordCipher`

Tái tạo từ class `N` trong Zalo PC:

1. **Khóa hằng số:** `PW_KEY = "9fc5vEW3"`.
2. **Băm SHA-256:**
   $$\text{raw\_hash} = \text{SHA-256}(\text{"\{PW\_KEY\}-\{phone\}-\{imei\}-\{salt\}"}).\text{hex}().\text{upper}()$$
3. **Dẫn xuất khóa AES:**
   $$\text{enk} = \text{even}(\text{raw\_hash})[0..16] + \text{reverse}(\text{odd}(\text{raw\_hash}))[0..16]$$
4. **Chuỗi dữ liệu mật khẩu:**
   $$\text{data} = \text{salt} + \text{reverse}(\text{password})$$
5. **Mã hóa:** AES-256-CBC (khóa `enk`, zero IV, PKCS7) -> Base64 -> Bọc trong cấu trúc tham số ParamCipher v2.

---

## 6. Cơ chế Mã hóa Phiên `zpw_enk` và Token `zpw_sek`

- **`zpw_sek` (Session Encryption Key / Cookie):**
  - Trả về trong header `Set-Cookie` của request xác nhận QR hoặc trong body JSON.
  - Phải được đính kèm trong mọi request qua header:
    - `Cookie: zpw_sek={sek}; _zlang=vi`
    - `zpw_sek: {sek}`
    - `Origin: https://chat.zalo.me`
    - `Referer: https://chat.zalo.me/`
- **`zpw_enk` (Data Decryption Key):**
  - Trả về từ API `getLoginInfo`.
  - Chuỗi Base64 độ dài 24 ký tự giải mã ra mảng **16 bytes** (Chuẩn **AES-128-CBC**, zero IV).
  - Dùng để mã hóa tham số `params` và giải mã toàn bộ dữ liệu trả về từ `getfriends`, `get-last-msgs`, `getlg/v4`, `getminiprofiles`.
