//! Adversarial stress test harness for Milestone M1 (Download Folder Selection - Requirement R1).
//! Validates path handling, unicode fidelity, directory creation, collision avoidance,
//! security traversal protection, and settings serialization.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static FIXTURE_SEQ: AtomicU64 = AtomicU64::new(0);

struct TempDirFixture {
    path: PathBuf,
}

impl TempDirFixture {
    fn new(prefix: &str) -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = FIXTURE_SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("{prefix}_{}_{nonce}_{seq}", std::process::id()));
        fs::create_dir_all(&path).expect("Failed to create temporary test root directory");
        Self { path }
    }
}

impl Drop for TempDirFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(default)]
struct TestAppSettings {
    download_dir: String,
    language: String,
    theme_mode: String,
}

impl Default for TestAppSettings {
    fn default() -> Self {
        let default_dl = std::env::var("USERPROFILE")
            .ok()
            .map(|p| format!(r"{p}\Downloads\Zalo Received Files"))
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| format!("{h}/Downloads/Zalo Received Files"))
            })
            .unwrap_or_else(|| "Downloads/Zalo Received Files".into());

        Self {
            download_dir: default_dl,
            language: "Tiếng Việt".into(),
            theme_mode: "dark".into(),
        }
    }
}

/// Adversarial paths matrix: spaces, Vietnamese unicode, accents, emojis,
/// deep nesting, trailing slashes, forward slashes, and mixed slashes.
const ADVERSARIAL_SUBPATHS: &[&str] = &[
    // 1. Plain spaces
    "Standard Folder With Spaces/Downloads Received",
    // 2. Vietnamese unicode with accents and special tones
    "Thư mục tải về Zalo - Tiếng Việt có dấu/Tệp đính kèm người gửi",
    // 3. Multilingual accents (French, German, Nordic)
    "Café_naïve_München_Über_Ångström/Sub_Détails",
    // 4. Emojis and unicode symbols
    "📁_Zalo_Downloads_🔥_🚀/📦_Special_Archive",
    // 5. Deeply nested directory hierarchy (12 levels deep)
    "d1/d2/d3/d4/d5/d6/d7/d8/d9/d10/d11/d12",
    // 6. Trailing forward slash
    "folder_trailing_slash/",
    // 7. Trailing backslash
    "folder_trailing_backslash\\",
    // 8. Mixed forward and backslashes
    "mixed_slashes/part1\\part2/part3\\final",
    // 9. Bracketed and punctuated names
    "[Zalo] (Downloads) - {2026}.v1.0 + Extra",
];

#[test]
fn test_adversarial_paths_directory_creation_and_file_io() {
    let fixture = TempDirFixture::new("m1_adversarial_dirs");

    for &subpath in ADVERSARIAL_SUBPATHS {
        let target_dir = fixture.path.join(subpath);

        // 1. Test recursive creation
        fs::create_dir_all(&target_dir)
            .unwrap_or_else(|e| panic!("create_dir_all failed for subpath '{subpath}': {e}"));

        assert!(
            target_dir.is_dir(),
            "Path should be recognized as a valid directory: {:?}",
            target_dir
        );

        // 2. Test writing and reading data
        let test_file = target_dir.join("test_file_đính_kèm.bin");
        let payload = b"Empirical verification of write/read roundtrip in adversarial folder";
        fs::write(&test_file, payload)
            .unwrap_or_else(|e| panic!("fs::write failed at {:?}: {e}", test_file));

        let read_back = fs::read(&test_file)
            .unwrap_or_else(|e| panic!("fs::read failed at {:?}: {e}", test_file));
        assert_eq!(read_back, payload, "Payload mismatch in {:?}", test_file);

        // 3. Test idempotent create_dir_all on existing directory
        assert!(
            fs::create_dir_all(&target_dir).is_ok(),
            "create_dir_all on existing dir must be idempotent"
        );
    }
}

#[test]
fn test_app_settings_json_roundtrip_fidelity_with_adversarial_paths() {
    let fixture = TempDirFixture::new("m1_settings_roundtrip");

    for (idx, &subpath) in ADVERSARIAL_SUBPATHS.iter().enumerate() {
        let full_path = fixture.path.join(subpath).display().to_string();

        let original = TestAppSettings {
            download_dir: full_path.clone(),
            language: "Tiếng Việt".into(),
            theme_mode: "light".into(),
        };

        // 1. Serialize to JSON
        let json_str = serde_json::to_string_pretty(&original)
            .expect("Failed to serialize settings with adversarial path");

        // 2. Write to disk (simulating AppSettings::save)
        let config_file = fixture.path.join(format!("app_settings_{idx}.json"));
        fs::write(&config_file, &json_str).expect("Failed to write settings json");

        // 3. Read from disk and deserialize (simulating AppSettings::load)
        let read_str = fs::read_to_string(&config_file).expect("Failed to read settings json");
        let deserialized: TestAppSettings =
            serde_json::from_str(&read_str).expect("Failed to deserialize settings json");

        assert_eq!(
            deserialized.download_dir, original.download_dir,
            "Deserialized download_dir must match exactly (preserving unicode, slashes, and spaces)"
        );
        assert_eq!(deserialized, original);
    }
}

#[test]
fn test_app_settings_missing_download_dir_uses_default() {
    let partial_json = r#"{"language": "English", "theme_mode": "dark"}"#;
    let deserialized: TestAppSettings =
        serde_json::from_str(partial_json).expect("Should deserialize partial json with defaults");

    assert!(
        !deserialized.download_dir.is_empty(),
        "Missing download_dir must fallback to default"
    );
    assert!(
        deserialized.download_dir.contains("Zalo Received Files"),
        "Default download_dir must point to Zalo Received Files"
    );
}

#[test]
fn test_download_worker_attachment_path_deduplication_and_sanitization() {
    let fixture = TempDirFixture::new("m1_worker_download_sim");
    let target_dir = fixture.path.join("Thư mục tải về Zalo (VN)/Tệp");
    fs::create_dir_all(&target_dir).expect("Failed to create target download dir");

    // Test Case 1: Traversal attack in filename
    let traversal_names = &[
        "../../evil.exe",
        "..\\..\\system32_calc.bat",
        "/etc/passwd",
        "C:\\Windows\\System32\\cmd.exe",
    ];

    for &raw_name in traversal_names {
        let clean_name = zalo_rust::chat::resolve_attachment_file_name(
            Some(raw_name),
            raw_name,
            "https://example.com/download?id=123",
        );

        let dest_path = target_dir.join(&clean_name);

        // Crucial security check: Destination path MUST remain inside target_dir!
        assert!(
            dest_path.starts_with(&target_dir),
            "Resolved attachment name '{clean_name}' (from '{raw_name}') escaped target dir: {:?}",
            dest_path
        );
        assert!(
            !clean_name.contains('/') && !clean_name.contains('\\'),
            "Resolved attachment name must not contain path separators: '{clean_name}'"
        );
    }

    // Test Case 2: Deduplication collision avoidance (matching worker.rs logic)
    let base_name = "Báo cáo tài chính Q4.docx";
    let bytes1 = b"Report content version 1";
    let bytes2 = b"Report content version 2";
    let bytes3 = b"Report content version 3";

    let simulate_save = |bytes: &[u8]| -> (String, PathBuf) {
        let clean_name = zalo_rust::chat::resolve_attachment_file_name(
            Some(base_name),
            base_name,
            "https://example.com/file",
        );

        let (stem, ext) = match clean_name.rfind('.') {
            Some(dot_idx) if dot_idx > 0 && dot_idx + 1 < clean_name.len() => {
                (&clean_name[..dot_idx], Some(&clean_name[dot_idx + 1..]))
            }
            _ => (clean_name.as_str(), None),
        };

        let mut dest_path = target_dir.join(&clean_name);
        let mut final_name = clean_name.clone();
        let mut suffix_idx = 1_u32;
        while dest_path.exists() {
            final_name = match ext {
                Some(e) => format!("{stem} ({suffix_idx}).{e}"),
                None => format!("{stem} ({suffix_idx})"),
            };
            dest_path = target_dir.join(&final_name);
            suffix_idx += 1;
        }

        fs::write(&dest_path, bytes).expect("Failed to write attachment");
        (final_name, dest_path)
    };

    let (name1, path1) = simulate_save(bytes1);
    assert_eq!(name1, "Báo cáo tài chính Q4.docx");
    assert!(path1.exists());
    assert_eq!(fs::read(&path1).unwrap(), bytes1);

    let (name2, path2) = simulate_save(bytes2);
    assert_eq!(name2, "Báo cáo tài chính Q4 (1).docx");
    assert!(path2.exists());
    assert_eq!(fs::read(&path2).unwrap(), bytes2);

    let (name3, path3) = simulate_save(bytes3);
    assert_eq!(name3, "Báo cáo tài chính Q4 (2).docx");
    assert!(path3.exists());
    assert_eq!(fs::read(&path3).unwrap(), bytes3);

    // Verify all 3 files remain intact and distinct
    assert_eq!(fs::read(&path1).unwrap(), bytes1);
    assert_eq!(fs::read(&path2).unwrap(), bytes2);
    assert_eq!(fs::read(&path3).unwrap(), bytes3);
}

#[test]
fn test_path_normalization_trailing_and_forward_slashes() {
    let fixture = TempDirFixture::new("m1_slash_norm");

    // Trailing slash joins
    let p_trailing = fixture.path.join("subfolder\\");
    let joined_file = p_trailing.join("photo.png");

    fs::create_dir_all(&p_trailing).expect("Failed to create dir with trailing slash");
    fs::write(&joined_file, b"image data").expect("Failed to write to joined path");

    assert!(joined_file.exists());
    assert_eq!(fs::read(&joined_file).unwrap(), b"image data");

    // Forward slash representation on Windows
    let forward_slash_str = fixture.path.to_str().unwrap().replace('\\', "/");
    let p_forward = PathBuf::from(format!("{forward_slash_str}/forward_sub"));
    fs::create_dir_all(&p_forward).expect("Failed to create dir from forward slash PathBuf");
    assert!(p_forward.is_dir());
}

#[cfg(target_os = "windows")]
#[test]
fn test_windows_com_dialog_apartment_threading_stress() {
    // Stress test COM STA initialization and cleanup repeatedly across threads
    // ensuring no thread-state corruption or leak occurs
    for _ in 0..5 {
        let handle = std::thread::spawn(|| {
            #[link(name = "ole32")]
            extern "system" {
                fn CoInitializeEx(pv_reserved: *mut std::ffi::c_void, dw_co_init: u32) -> i32;
                fn CoUninitialize();
            }
            // COINIT_APARTMENTTHREADED (0x2) | COINIT_DISABLE_OLE1DDE (0x4)
            let hr = unsafe { CoInitializeEx(std::ptr::null_mut(), 0x2 | 0x4) };
            assert!(hr >= 0, "CoInitializeEx in worker thread must succeed (hr: {hr:#x})");
            unsafe { CoUninitialize() };
        });
        handle.join().expect("Worker thread panicked during COM test");
    }
}
