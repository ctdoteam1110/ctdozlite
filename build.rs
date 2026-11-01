//! Embed `src/assets/app_icon.ico` into the Windows executable.
//!
//! Build scripts are compiled for the *host*, so `#[cfg(target_os = "windows")]` here would
//! describe the machine running Cargo, not the binary being produced. The target is read from
//! `CARGO_CFG_TARGET_OS` / `CARGO_CFG_TARGET_ENV` instead, which also makes cross-compiling
//! from Linux behave correctly.
//!
//! * `*-windows-msvc`: a standard 32-bit `.res` file (`RT_ICON` + `RT_GROUP_ICON`) is written
//!   in pure Rust and passed to `link.exe` / `lld-link` — no `rc.exe` required.
//! * `*-windows-gnu`: a one-line `.rc` script is compiled to a COFF object with `windres`
//!   (`$WINDRES`, `<arch>-w64-mingw32-windres`, or `windres`). Skipped with a warning when no
//!   `windres` is available.
//!
//! Failing to embed the icon never fails the build.

use std::path::{Path, PathBuf};

const ICON_PATH: &str = "src/assets/app_icon.ico";
const BIN_NAME: &str = "zalo-rust";

type BoxError = Box<dyn std::error::Error>;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={ICON_PATH}");
    println!("cargo:rerun-if-changed=src/assets/app_icon.png");
    println!("cargo:rerun-if-env-changed=WINDRES");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let result = match target_env.as_str() {
        "msvc" => embed_icon_msvc(),
        "gnu" | "gnullvm" => embed_icon_gnu(),
        other => Err(format!("unsupported Windows target environment `{other}`").into()),
    };
    if let Err(e) = result {
        println!("cargo:warning=Could not embed Windows icon resource: {e}");
    }
}

fn out_dir() -> Result<PathBuf, BoxError> {
    Ok(PathBuf::from(std::env::var("OUT_DIR")?))
}

/// MSVC: write a `.res` file by hand and hand it to the linker.
fn embed_icon_msvc() -> Result<(), BoxError> {
    let ico = std::fs::read(ICON_PATH)?;
    let res = ico_to_res(&ico)?;
    let res_path = out_dir()?.join("app_icon.res");
    std::fs::write(&res_path, &res)?;
    println!("cargo:rustc-link-arg-bin={BIN_NAME}={}", res_path.display());
    Ok(())
}

/// GNU (MinGW): compile a tiny `.rc` with `windres` into a COFF object and link it.
fn embed_icon_gnu() -> Result<(), BoxError> {
    let out = out_dir()?;
    let icon_abs = std::fs::canonicalize(ICON_PATH)?;
    // `windres` accepts forward slashes on every host; escape backslashes just in case.
    let icon_str = icon_abs.display().to_string().replace('\\', "/");
    let rc_path = out.join("app_icon.rc");
    std::fs::write(&rc_path, format!("1 ICON \"{icon_str}\"\n"))?;
    let obj_path = out.join("app_icon_res.o");

    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_else(|_| "x86_64".into());
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(w) = std::env::var("WINDRES") {
        if !w.is_empty() {
            candidates.push(w);
        }
    }
    candidates.push(format!("{arch}-w64-mingw32-windres"));
    candidates.push("windres".into());

    let mut last_err = String::from("no windres candidate");
    for tool in &candidates {
        match run_windres(tool, &rc_path, &obj_path) {
            Ok(()) => {
                println!("cargo:rustc-link-arg-bin={BIN_NAME}={}", obj_path.display());
                return Ok(());
            }
            Err(e) => last_err = format!("{tool}: {e}"),
        }
    }
    Err(format!("windres not available ({last_err})").into())
}

fn run_windres(tool: &str, rc: &Path, obj: &Path) -> Result<(), BoxError> {
    let status = std::process::Command::new(tool)
        .arg("-i")
        .arg(rc)
        .arg("-O")
        .arg("coff")
        .arg("-o")
        .arg(obj)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("exited with {status}").into())
    }
}

/// Convert an `.ico` file into a 32-bit `.res` file holding `RT_ICON` entries (ordinals
/// `1..=count`) and one `RT_GROUP_ICON` (ordinal 1).
fn ico_to_res(ico: &[u8]) -> Result<Vec<u8>, BoxError> {
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
        if img_off.checked_add(bytes_in_res).map_or(true, |end| end > ico.len()) {
            return Err("ICO image slice out of bounds".into());
        }
        let icon_id = (i + 1) as u16;

        // GRPICONDIRENTRY = first 12 bytes of ICONDIRENTRY + u16 resource id
        grp_dir.extend_from_slice(&entry[0..12]);
        grp_dir.extend_from_slice(&icon_id.to_le_bytes());

        write_res_header(&mut res, bytes_in_res as u32, 3, icon_id, 0x1010);
        res.extend_from_slice(&ico[img_off..img_off + bytes_in_res]);
        align4(&mut res);
    }

    // 3. RT_GROUP_ICON (type = 14) with ordinal 1
    write_res_header(&mut res, grp_dir.len() as u32, 14, 1, 0x1030);
    res.extend_from_slice(&grp_dir);
    align4(&mut res);
    Ok(res)
}

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

fn align4(buf: &mut Vec<u8>) {
    while buf.len() % 4 != 0 {
        buf.push(0);
    }
}
