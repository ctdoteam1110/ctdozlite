//! Local, temporary Zalo debugger used only to recover a verified backup key.

/// Select a candidate only if it decrypts the selected archive header.
pub fn verified_candidate(response: &serde_json::Value, header: &[u8]) -> Option<zeroize::Zeroizing<String>> {
    response.pointer("/result/result/value")?.as_array()?.iter().filter_map(|v| v.as_str()).find(|v| crate::backup::verify_v2_header(header, v).is_ok()).map(|v| zeroize::Zeroizing::new(v.to_owned()))
}

#[cfg(target_os = "windows")]
struct DebugJob(isize);

#[cfg(target_os = "windows")]
impl DebugJob {
    fn start(exe: &std::path::Path, port: u16) -> Result<Self, String> {
        use std::{mem::{size_of, zeroed}, os::windows::ffi::OsStrExt, ptr::null};
        use windows_sys::Win32::{Foundation::CloseHandle, System::{JobObjects::{AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject, JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE}, Threading::{CreateProcessW, ResumeThread, TerminateProcess, CREATE_SUSPENDED, PROCESS_INFORMATION, STARTUPINFOW}}};
        // Create suspended so no descendant can escape before assignment; the job kills all members when its last handle closes.
        unsafe {
            let job = CreateJobObjectW(null(), null());
            if job.is_null() { return Err("Không thể cách ly phiên Zalo tạm thời".into()); }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(job, JobObjectExtendedLimitInformation, &limits as *const _ as *const _, size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32) == 0 {
                CloseHandle(job); return Err("Không thể thiết lập bảo vệ phiên Zalo".into());
            }
            let application: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
            let command = format!("\"{}\" --remote-debugging-address=127.0.0.1 --remote-debugging-port={port}", exe.display());
            let mut command: Vec<u16> = command.encode_utf16().chain(Some(0)).collect();
            let mut startup: STARTUPINFOW = zeroed(); startup.cb = size_of::<STARTUPINFOW>() as u32;
            let mut info: PROCESS_INFORMATION = zeroed();
            if CreateProcessW(application.as_ptr(), command.as_mut_ptr(), null(), null(), 0, CREATE_SUSPENDED, null(), null(), &startup, &mut info) == 0 {
                CloseHandle(job); return Err("Không thể mở Zalo Desktop".into());
            }
            if AssignProcessToJobObject(job, info.hProcess) == 0 {
                TerminateProcess(info.hProcess, 1); CloseHandle(info.hThread); CloseHandle(info.hProcess); CloseHandle(job);
                return Err("Không thể cách ly Zalo; hãy dùng hướng dẫn thủ công".into());
            }
            if ResumeThread(info.hThread) == u32::MAX {
                CloseHandle(info.hThread); CloseHandle(info.hProcess); CloseHandle(job);
                return Err("Không thể khởi động phiên Zalo tạm thời".into());
            }
            CloseHandle(info.hThread); CloseHandle(info.hProcess);
            Ok(Self(job as isize))
        }
    }
    fn close(mut self) -> bool {
        let handle = std::mem::replace(&mut self.0, 0);
        unsafe { windows_sys::Win32::Foundation::CloseHandle(handle as _) != 0 }
    }
}

#[cfg(target_os = "windows")]
impl Drop for DebugJob {
    fn drop(&mut self) {
        if self.0 != 0 { unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0 as _); } }
    }
}

#[cfg(target_os = "windows")]
pub async fn recover(path: std::path::PathBuf, cancel: &'static std::sync::atomic::AtomicBool) -> Result<zeroize::Zeroizing<String>, String> {
    use futures_util::{SinkExt, StreamExt};
    use std::{io::Read, os::windows::process::CommandExt, sync::atomic::Ordering, time::Duration};
    use tokio_tungstenite::tungstenite::Message;
    const NO_WINDOW: u32 = 0x08000000;
    let output = std::process::Command::new("tasklist.exe").args(["/FI", "IMAGENAME eq Zalo.exe", "/NH"]).creation_flags(NO_WINDOW).output().map_err(|_| "Không thể kiểm tra Zalo Desktop".to_string())?;
    if !output.status.success() { return Err("Không thể kiểm tra Zalo Desktop đang chạy".into()); }
    if String::from_utf8_lossy(&output.stdout).to_ascii_lowercase().contains("zalo.exe") {
        return Err("Hãy thoát Zalo hoàn toàn từ khay hệ thống, sau đó thử lại".into());
    }
    let exe = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("Không tìm thấy thư mục ứng dụng")?).join("Programs").join("Zalo").join("Zalo.exe");
    if !exe.is_file() { return Err("Không tìm thấy Zalo Desktop tại vị trí cài đặt mặc định".into()); }
    let mut header = [0u8; 512];
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut header)).map_err(|_| "Không thể đọc bản sao lưu".to_string())?;
    let port = std::net::TcpListener::bind("127.0.0.1:0").and_then(|s| s.local_addr()).map_err(|_| "Không thể mở cổng cục bộ".to_string())?.port();
    let client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(2)).build().map_err(|_| "Không thể khởi tạo kết nối cục bộ")?;
    let job = match DebugJob::start(&exe, port) { Ok(job) => job, Err(error) => { let _ = std::process::Command::new(&exe).spawn(); return Err(error); } };
    let endpoint = format!("http://127.0.0.1:{port}/json/list");
    let expression = r#"(()=>{const a=document.getElementById('app');if(!a)return [];const k=Object.keys(a).find(x=>x.startsWith('__reactContainer'));const q=[a[k]],s=new Set(),v=new Set();while(q.length&&s.size<30000){const f=q.pop();if(!f||s.has(f))continue;s.add(f);for(const x of [f.memoizedProps?.user?.UIN,f.stateNode?.fetcher?.UIN,f.stateNode?.storage?.UIN])if(typeof x==='string'&&x.length===32)v.add(x);for(const n of [f.child,f.sibling,f.alternate])if(n)q.push(n)}return [...v]})()"#;
    let result = async {
    for _ in 0..60 {
        if cancel.load(Ordering::SeqCst) { return Err("Đã hủy tìm khóa".into()); }
        if let Ok(resp) = client.get(&endpoint).send().await {
            if let Ok(items) = resp.json::<serde_json::Value>().await {
                if let Some(targets) = items.as_array() {
                    for target in targets {
                        if target.get("type").and_then(|x| x.as_str()) != Some("page") || !target.get("url").and_then(|x| x.as_str()).unwrap_or("").contains("index.html") { continue; }
                        let Some(ws) = target.get("webSocketDebuggerUrl").and_then(|x| x.as_str()) else { continue; };
                        if !ws.starts_with(&format!("ws://127.0.0.1:{port}/devtools/page/")) { continue; }
                        let Ok(Ok((mut socket, _))) = tokio::time::timeout(Duration::from_secs(2), tokio_tungstenite::connect_async(ws)).await else { continue; };
                        let request = serde_json::json!({"id":1,"method":"Runtime.evaluate","params":{"expression":expression,"returnByValue":true}}).to_string();
                        if socket.send(Message::Text(request.into())).await.is_err() { continue; }
                        if let Ok(Some(Ok(Message::Text(text)))) = tokio::time::timeout(Duration::from_secs(2), socket.next()).await {
                            if let Ok(response) = serde_json::from_str::<serde_json::Value>(&text) {
                                if let Some(value) = verified_candidate(&response, &header) { return Ok(value); }
                            }
                        }
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err("Không tìm thấy khóa phù hợp. Hãy bảo đảm Zalo đã đăng nhập cùng tài khoản và thử lại".into())
    }.await;
    // Close the temporary browser through its own debugger before restoring Zalo.
    let version_endpoint = format!("http://127.0.0.1:{port}/json/version");
    if let Ok(response) = client.get(version_endpoint).send().await {
        if let Ok(version) = response.json::<serde_json::Value>().await {
            if let Some(ws) = version.get("webSocketDebuggerUrl").and_then(|v| v.as_str()) {
                if ws.starts_with(&format!("ws://127.0.0.1:{port}/devtools/browser/")) {
                    if let Ok(Ok((mut socket, _))) = tokio::time::timeout(Duration::from_secs(2), tokio_tungstenite::connect_async(ws)).await {
                        let _ = socket.send(Message::Text(r#"{"id":1,"method":"Browser.close"}"#.into())).await;
                    }
                }
            }
        }
    }
    let closed = job.close();
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut stopped = false;
    for _ in 0..50 {
        if std::net::TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_err() { stopped = true; break; }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    if !closed || !stopped { return Err("Không thể xác nhận phiên kiểm tra Zalo đã đóng; hãy khởi động lại máy trước khi thử lại".into()); }
    std::process::Command::new(&exe).spawn().map_err(|_| "Đã đóng phiên kiểm tra nhưng không thể mở lại Zalo bình thường".to_string())?;
    result
}

#[cfg(not(target_os = "windows"))]
pub async fn recover(_: std::path::PathBuf, _: &'static std::sync::atomic::AtomicBool) -> Result<zeroize::Zeroizing<String>, String> {
    Err("Tự tìm khóa hiện chỉ hỗ trợ Windows".into())
}
