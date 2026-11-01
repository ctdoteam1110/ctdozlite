# Hướng dẫn cài đặt QTzalo

Mọi bản dựng sẵn nằm tại [GitHub Releases](https://github.com/quangthoai1985/QTzalo/releases). Các file đều do GitHub Actions build trực tiếp từ mã nguồn của commit được ghi trong trang Release.

## 1. Kiểm tra file tải về (khuyến nghị)

Mỗi Release có file `SHA256SUMS.txt`. So sánh mã băm trước khi chạy:

Windows (PowerShell):

```powershell
Get-FileHash .\QTzalo-0.1.0-setup-windows-x86_64.exe -Algorithm SHA256
```

Linux / macOS:

```bash
sha256sum -c SHA256SUMS.txt --ignore-missing
```

Giá trị phải trùng khớp với dòng tương ứng trong `SHA256SUMS.txt`.

## 2. Windows 10 / 11 (x64)

### Bộ cài (khuyến nghị)

1. Tải `QTzalo-<phiên bản>-setup-windows-x86_64.exe`.
2. Chạy file. Mặc định bộ cài **không cần quyền Administrator** và cài vào `%LOCALAPPDATA%\Programs\QTzalo`. Nếu muốn cài cho mọi người dùng, chọn tuỳ chọn tương ứng ở hộp thoại đầu tiên.
3. Có thể tick *Create a desktop shortcut* để tạo biểu tượng ngoài màn hình.
4. Mở **QTzalo** từ Start Menu, quét mã QR bằng ứng dụng Zalo trên điện thoại và bấm xác nhận.

### Cảnh báo SmartScreen

Bộ cài chưa được ký số nên Windows có thể hiện *"Windows protected your PC"*. Sau khi đã kiểm tra SHA256 ở bước 1:

1. Bấm **More info** (Thông tin thêm).
2. Bấm **Run anyway** (Vẫn chạy).

### Bản portable

`QTzalo-<phiên bản>-portable-windows-x86_64.exe` chạy trực tiếp, không cần cài. Dữ liệu vẫn được lưu ở `%APPDATA%\QTzalo`, giống bản cài.

### Cập nhật và gỡ cài đặt

- **Cập nhật:** chạy bộ cài của phiên bản mới. Bộ cài nhận ra bản cũ và cài đè, dữ liệu được giữ nguyên.
- **Gỡ cài đặt:** *Settings → Apps → QTzalo → Uninstall*. Cuối quá trình gỡ, bộ cài hỏi có xoá luôn dữ liệu người dùng (`%APPDATA%\QTzalo`) không. Mặc định là **không**.

## 3. Linux

### Ubuntu 22.04+ / Debian 12+ / Linux Mint 21+

```bash
sudo apt install ./qtzalo_0.1.0_amd64.deb
```

Chạy bằng lệnh `qtzalo` hoặc mở **QTzalo** từ menu ứng dụng. Gỡ bằng `sudo apt remove qtzalo`.

### Các bản phân phối khác (Fedora, Arch, openSUSE, …)

```bash
tar -xzf qtzalo-0.1.0-linux-x86_64.tar.gz
cd qtzalo-0.1.0-linux-x86_64
./install.sh
```

- `./install.sh` cài vào `/usr/local` (dùng `sudo` nếu có). Thêm `--user` để cài vào `~/.local` mà không cần quyền root.
- Gỡ: `./uninstall.sh`. Thêm `--purge` để xoá cả dữ liệu người dùng.

Bản Linux được build trên Ubuntu 22.04 nên cần `glibc ≥ 2.35`.

### Gói nên cài thêm

| Mục đích | Debian/Ubuntu | Fedora |
|---|---|---|
| Hiển thị tiếng Việt và chữ Hán | `fonts-noto-core fonts-noto-cjk` | `google-noto-sans-fonts google-noto-sans-cjk-fonts` |
| Hộp thoại chọn file | `zenity` | `zenity` |
| Dán ảnh từ clipboard | `xclip` hoặc `wl-clipboard` | `xclip` hoặc `wl-clipboard` |
| Gọi video | `ffmpeg` | `ffmpeg` (RPM Fusion) |

## 4. macOS (Apple Silicon)

Bản `.dmg` là bản thử nghiệm và chưa ký số. Mở `.dmg`, kéo **QTzalo** vào thư mục *Applications*. Lần đầu mở, bấm chuột phải vào ứng dụng, chọn **Open** rồi xác nhận.

## 5. Dữ liệu và đăng xuất

| Hệ điều hành | Thư mục dữ liệu |
|---|---|
| Windows | `%APPDATA%\QTzalo` |
| Linux | `~/.local/share/qtzalo` |
| macOS | `~/Library/Application Support/QTzalo` |

- Đăng xuất trong ứng dụng sẽ xoá phiên đăng nhập. Lịch sử tin nhắn cục bộ được giữ lại.
- Nếu nghi ngờ lộ file `session.json`, hãy đăng xuất phiên đó từ ứng dụng Zalo trên điện thoại: *Cài đặt → Tài khoản và bảo mật → Thiết bị đăng nhập*.

Gặp sự cố khác: xem [KHAC_PHUC_SU_CO.md](KHAC_PHUC_SU_CO.md).
