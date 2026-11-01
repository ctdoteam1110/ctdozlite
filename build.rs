//! Embed `src/assets/app_icon.ico` into the Windows PE executable (`RT_ICON` + `RT_GROUP_ICON`)
//! by emitting a standard COFF/Win32 `.res` file in `OUT_DIR` without requiring external `rc.exe`.

fn main() {
    println!("cargo:rerun-if-changed=src/assets/app_icon.ico");
    println!("cargo:rerun-if-changed=src/assets/app_icon.png");

    #[cfg(target_os = "windows")]
    {
        if let Err(e) = embed_windows_icon() {
            println!("cargo:warning=Could not embed Windows icon resource: {e}");
        }
    }
}

#[cfg(target_os = "windows")]
fn embed_windows_icon() -> Result<(), Box<dyn std::error::Error>> {
    use std::path::PathBuf;

    let ico = std::fs::read("src/assets/app_icon.ico")?;
    if ico.len() < 6 || u16::from_le_bytes([ico[0], ico[1]]) != 0 || u16::from_le_bytes([ico[2], ico[3]]) != 1 {
        return Err("Invalid ICO header".into());
    }
    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    if ico.len() < 6 + count * 16 {
        return Err("Truncated ICO directory".into());
    }

    let mut res = Vec::with_capacity(ico.len() + 512);

    // 1. 32-byte null resource header required at the start of every 32-bit .res file
    res.extend_from_slice(&0u32.to_le_bytes()); // DataSize = 0
    res.extend_from_slice(&32u32.to_le_bytes()); // HeaderSize = 32
    res.extend_from_slice(&[0xFF, 0xFF, 0x00, 0x00]); // Type = 0
    res.extend_from_slice(&[0xFF, 0xFF, 0x00, 0x00]); // Name = 0
    res.extend_from_slice(&[0u8; 16]);

    // 2. Emit each RT_ICON (type = 3) entry with ordinal 1..=count
    let mut grp_dir = Vec::with_capacity(6 + count * 14);
    grp_dir.extend_from_slice(&ico[0..6]); // idReserved, idType, idCount

    for i in 0..count {
        let entry_off = 6 + i * 16;
        let entry = &ico[entry_off..entry_off + 16];
        let bytes_in_res = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]) as usize;
        let img_off = u32::from_le_bytes([entry[12], entry[13], entry[14], entry[15]]) as usize;
        if img_off + bytes_in_res > ico.len() {
            return Err("ICO image slice out of bounds".into());
        }
        let icon_id = (i + 1) as u16;

        // Append GRPICONDIRENTRY (12 bytes from ICONDIRENTRY + u16 icon_id)
        grp_dir.extend_from_slice(&entry[0..12]);
        grp_dir.extend_from_slice(&icon_id.to_le_bytes());

        // Write RT_ICON resource header (32 bytes)
        write_res_header(&mut res, bytes_in_res as u32, 3, icon_id, 0x1010);
        res.extend_from_slice(&ico[img_off..img_off + bytes_in_res]);
        align4(&mut res);
    }

    // 3. Emit RT_GROUP_ICON (type = 14) with ordinal 1 ("MAINICON")
    write_res_header(&mut res, grp_dir.len() as u32, 14, 1, 0x1030);
    res.extend_from_slice(&grp_dir);
    align4(&mut res);

    // 4. Emit RT_VERSION (type = 16, ID = 1) with CompanyName, FileDescription, ProductName, FileVersion
    // so Windows Defender / SmartScreen sees a complete PE VERSIONINFO block.
    let ver_data = build_vs_version_info();
    write_res_header(&mut res, ver_data.len() as u32, 16, 1, 0x0030);
    res.extend_from_slice(&ver_data);
    align4(&mut res);

    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
    let res_path = out_dir.join("app_icon.res");
    std::fs::write(&res_path, &res)?;

    // GNU ld cannot read a raw .res; windres (MinGW binutils) converts it to a COFF object.
    let link_path = if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu") {
        let obj_path = out_dir.join("app_icon.o");
        let ok = std::process::Command::new("windres")
            .args(["-J", "res", "-O", "coff", "-i"])
            .arg(&res_path)
            .arg("-o")
            .arg(&obj_path)
            .status()?
            .success();
        if !ok {
            return Err("windres failed".into());
        }
        obj_path
    } else {
        res_path
    };

    println!("cargo:rustc-link-arg-bin=ctdozlite={}", link_path.display());
    Ok(())
}

#[cfg(target_os = "windows")]
fn write_res_header(buf: &mut Vec<u8>, data_size: u32, res_type: u16, res_name: u16, mem_flags: u16) {
    buf.extend_from_slice(&data_size.to_le_bytes());
    buf.extend_from_slice(&32u32.to_le_bytes()); // HeaderSize = 32
    buf.extend_from_slice(&0xFFFFu16.to_le_bytes());
    buf.extend_from_slice(&res_type.to_le_bytes());
    buf.extend_from_slice(&0xFFFFu16.to_le_bytes());
    buf.extend_from_slice(&res_name.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes()); // DataVersion
    buf.extend_from_slice(&mem_flags.to_le_bytes());
    buf.extend_from_slice(&0x0409u16.to_le_bytes()); // LanguageId (en-US)
    buf.extend_from_slice(&0u32.to_le_bytes()); // Version
    buf.extend_from_slice(&0u32.to_le_bytes()); // Characteristics
}

#[cfg(target_os = "windows")]
fn align4(buf: &mut Vec<u8>) {
    while buf.len() % 4 != 0 {
        buf.push(0);
    }
}

#[cfg(target_os = "windows")]
fn build_vs_version_info() -> Vec<u8> {
    fn push_wstr(buf: &mut Vec<u8>, s: &str) {
        for u in s.encode_utf16() {
            buf.extend_from_slice(&u.to_le_bytes());
        }
        buf.extend_from_slice(&0u16.to_le_bytes());
    }

    fn make_node(key: &str, value: &[u8], is_text: bool, children: &[Vec<u8>]) -> Vec<u8> {
        let mut node = Vec::new();
        node.extend_from_slice(&0u16.to_le_bytes()); // wLength placeholder
        let w_val_len = if is_text {
            (value.len() / 2) as u16
        } else {
            value.len() as u16
        };
        node.extend_from_slice(&w_val_len.to_le_bytes());
        node.extend_from_slice(&(if is_text { 1u16 } else { 0u16 }).to_le_bytes());
        push_wstr(&mut node, key);
        align4(&mut node);
        node.extend_from_slice(value);
        align4(&mut node);
        for ch in children {
            node.extend_from_slice(ch);
            align4(&mut node);
        }
        let total_len = node.len() as u16;
        node[0..2].copy_from_slice(&total_len.to_le_bytes());
        node
    }

    fn make_str_pair(k: &str, v: &str) -> Vec<u8> {
        let mut val_bytes = Vec::new();
        push_wstr(&mut val_bytes, v);
        make_node(k, &val_bytes, true, &[])
    }

    let ver_str = env!("CARGO_PKG_VERSION");
    let mut parts = ver_str.split('.').map(|x| x.parse::<u16>().unwrap_or(0));
    let maj = parts.next().unwrap_or(0);
    let min = parts.next().unwrap_or(0);
    let pat = parts.next().unwrap_or(3);

    // VS_FIXEDFILEINFO (52 bytes)
    let mut ffi = Vec::with_capacity(52);
    ffi.extend_from_slice(&0xFEEF04BDu32.to_le_bytes()); // dwSignature
    ffi.extend_from_slice(&0x00010000u32.to_le_bytes()); // dwStrucVersion
    ffi.extend_from_slice(&(((maj as u32) << 16) | (min as u32)).to_le_bytes()); // dwFileVersionMS
    ffi.extend_from_slice(&((pat as u32) << 16).to_le_bytes()); // dwFileVersionLS
    ffi.extend_from_slice(&(((maj as u32) << 16) | (min as u32)).to_le_bytes()); // dwProductVersionMS
    ffi.extend_from_slice(&((pat as u32) << 16).to_le_bytes()); // dwProductVersionLS
    ffi.extend_from_slice(&0x3Fu32.to_le_bytes()); // dwFileFlagsMask
    ffi.extend_from_slice(&0u32.to_le_bytes()); // dwFileFlags
    ffi.extend_from_slice(&0x00040004u32.to_le_bytes()); // dwFileOS = VOS_NT_WINDOWS32
    ffi.extend_from_slice(&0x00000001u32.to_le_bytes()); // dwFileType = VFT_APP
    ffi.extend_from_slice(&0u32.to_le_bytes()); // dwFileSubtype
    ffi.extend_from_slice(&0u32.to_le_bytes()); // dwFileDateMS
    ffi.extend_from_slice(&0u32.to_le_bytes()); // dwFileDateLS

    let str_entries = vec![
        make_str_pair("CompanyName", "ctdoteam1110"),
        make_str_pair("FileDescription", "ctdozlite Native Zalo Desktop Client"),
        make_str_pair("FileVersion", ver_str),
        make_str_pair("InternalName", "ctdozlite.exe"),
        make_str_pair("LegalCopyright", "Copyright (c) 2026 ctdoteam1110 (MIT OR Apache-2.0)"),
        make_str_pair("OriginalFilename", "ctdozlite.exe"),
        make_str_pair("ProductName", "ctdozlite"),
        make_str_pair("ProductVersion", ver_str),
    ];
    let str_table = make_node("040904B0", &[], true, &str_entries);
    let str_file_info = make_node("StringFileInfo", &[], true, &[str_table]);

    let trans_val = [0x09u8, 0x04, 0xB0, 0x04]; // 0x0409 (en-US), 1200 (UTF-16)
    let var_node = make_node("Translation", &trans_val, false, &[]);
    let var_file_info = make_node("VarFileInfo", &[], true, &[var_node]);

    make_node("VS_VERSION_INFO", &ffi, false, &[str_file_info, var_file_info])
}
