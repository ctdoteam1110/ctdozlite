# Cách bắt 1 request `loginByPasswv2` từ Zalo PC

Mục tiêu: lấy **một** request thật để so với bản Rust đang dựng.
Không cần bắt toàn bộ phiên — chỉ cần request lúc bấm Đăng nhập.

> Dùng tài khoản clone `0836553142` cho lần bắt này.

---

## Cách A — mitmproxy (khuyến nghị, có sẵn nếu bạn dùng Python)

```powershell
pip install mitmproxy
mitmdump -w zalo.flow --set confdir=~/.mitmproxy
```

1. Bật proxy hệ thống → `127.0.0.1:8080`
2. Mở `http://mitm.it` trên trình duyệt, cài cert mitmproxy (chọn Windows)
3. Mở Zalo PC → nhập số điện thoại + mật khẩu → bấm Đăng nhập
4. Đóng mitmdump (Ctrl+C)
5. Lọc request:

```powershell
mitmdump -nr zalo.flow --set flow_detail=0 2>&1 | Select-String loginByPasswv2
```

Nếu thấy request, xuất nó ra text:

```powershell
mitmdump -nr zalo.flow "~u loginByPasswv2" -w only.flow
```

Rồi copy **URL đầy đủ** (phần sau `POST `) vào `captured.txt`.

---

## Cách B — Fiddler Classic (GUI, dễ nhìn)

1. Cài Fiddler Classic, bật HTTPS decryption (Tools → Options → HTTPS → Decrypt)
2. Mở Zalo PC, đăng nhập
3. Trong Fiddler, filter `loginByPasswv2`
4. Chuột phải request → **Copy → Just Url**
5. Dán vào `captured.txt`

Fiddler tự cài cert vào Windows nên Zalo PC thường chấp nhận ngay.

---

## Cách C — không cài proxy: hook trong Zalo PC

Nếu Zalo PC bật được DevTools:

1. Mở Zalo PC → `Ctrl+Shift+I` (nếu bật được)
2. Vào tab Console, dán:

```js
const _fetch = window.fetch;
window.fetch = function(...a) {
  const u = typeof a[0] === 'string' ? a[0] : a[0].url;
  if (u && u.includes('loginByPasswv2')) {
    console.log('=== CAPTURED ===');
    console.log(u);
    copy(u);
  }
  return _fetch.apply(this, a);
};
```

3. Đăng nhập lại
4. URL đã nằm trong clipboard → dán vào `captured.txt`

Cách này không cần cert. Nhưng Zalo PC thường tắt DevTools ở bản phát hành, nên A hoặc B chắc ăn hơn.

---

## Sau khi có `captured.txt`

```powershell
cd F:\CODE\Reverse-engineer\DATA\4_zalo\zalo-rust
.\target\release\compare.exe
```

Nó in ra:
- mọi trường của request thật (đã decode)
- `signkey` thật, và biến thể nào khớp thuật toán của tôi
- payload đã giải mã — tức là **danh sách trường chính xác** server nhận
- `zcid` giải mã ra gì

Chỉ cần gửi tôi output đó. Không cần gửi mật khẩu (nó nằm trong `password` dưới dạng đã mã hoá).

---

## An toàn

- `captured.txt` chứa `zpw_sek`/session nếu bắt cả phiên — **chỉ bắt đúng 1 request login** là đủ.
- Đừng commit `captured.txt` hay `dump/` lên git.
- Xong việc thì đổi mật khẩu tài khoản clone.
