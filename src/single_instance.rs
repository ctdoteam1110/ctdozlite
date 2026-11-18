//! One GUI process per data folder.
//!
//! The first GUI takes an exclusive `flock` on `ctdozlite.lock` and listens on the Unix socket
//! `ctdozlite.sock`, both in the data folder. A second launch finds the lock held, asks the
//! running GUI to show its window through the socket, and exits. The TUI and headless modes
//! take no lock: they are run on purpose from a terminal, so a running GUI must not stop them.
//! On non-Unix targets this module is a no-op and every launch runs.

use std::path::Path;

/// File whose `flock` marks the running GUI. The kernel drops the lock when the holder exits
/// or crashes, so a file left behind never blocks the next launch.
pub const LOCK_FILE: &str = "ctdozlite.lock";
/// Unix socket the running GUI listens on for "show your window" requests.
pub const SOCKET_FILE: &str = "ctdozlite.sock";
/// Request a second launch sends to the running GUI.
const SHOW_REQUEST: &[u8] = b"show\n";

/// Result of [`acquire`].
pub enum Acquired {
    /// This process is the only GUI for the folder; keep the guard alive until exit.
    Primary(InstanceGuard),
    /// Another process holds the lock.
    AlreadyRunning,
}

/// Holds the instance lock (and, on Unix, the bound show-request socket).
pub struct InstanceGuard {
    #[cfg(unix)]
    _lock: std::fs::File,
    #[cfg(unix)]
    listener: Option<std::os::unix::net::UnixListener>,
}

/// Try to become the only GUI process for `dir`. Never blocks.
///
/// # Errors
/// Returns an error when the lock file cannot be opened or `flock` fails for a reason other
/// than another holder. Callers should fail open and start anyway.
#[cfg(unix)]
pub fn acquire(dir: &Path) -> std::io::Result<Acquired> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;

    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(dir.join(LOCK_FILE))?;
    // SAFETY: `lock` owns a valid open descriptor for the duration of the call.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let err = std::io::Error::last_os_error();
        return if err.kind() == std::io::ErrorKind::WouldBlock {
            Ok(Acquired::AlreadyRunning)
        } else {
            Err(err)
        };
    }
    // We hold the lock, so any socket file is left over from a dead instance.
    let socket = dir.join(SOCKET_FILE);
    let _ = std::fs::remove_file(&socket);
    // Fail open: without the socket a second launch cannot raise this window, but it is still
    // refused by the lock above.
    let listener = match std::os::unix::net::UnixListener::bind(&socket) {
        Ok(listener) => Some(listener),
        Err(err) => {
            crate::logger::log_error("single_instance::acquire", &err.to_string());
            None
        }
    };
    Ok(Acquired::Primary(InstanceGuard {
        _lock: lock,
        listener,
    }))
}

/// No-op on targets without `flock`: every launch is primary.
///
/// # Errors
/// Never fails.
#[cfg(not(unix))]
pub fn acquire(_dir: &Path) -> std::io::Result<Acquired> {
    Ok(Acquired::Primary(InstanceGuard {}))
}

/// Ask the GUI that holds the lock for `dir` to show its window.
///
/// # Errors
/// Returns an error when the running GUI is not listening (older build, or its socket failed).
#[cfg(unix)]
pub fn notify_running(dir: &Path) -> std::io::Result<()> {
    use std::io::Write;
    let mut stream = std::os::unix::net::UnixStream::connect(dir.join(SOCKET_FILE))?;
    stream.write_all(SHOW_REQUEST)
}

/// No-op on targets without the show socket.
///
/// # Errors
/// Never fails.
#[cfg(not(unix))]
pub fn notify_running(_dir: &Path) -> std::io::Result<()> {
    Ok(())
}

impl InstanceGuard {
    /// Serve show requests on a dedicated thread, calling `on_show` for each one. The thread
    /// blocks on `accept`, never on the UI thread (INV-3), and lives until the process exits.
    /// Requests sent before this call wait in the socket backlog.
    pub fn spawn_show_listener(&mut self, on_show: impl Fn() + Send + 'static) {
        #[cfg(unix)]
        if let Some(listener) = self.listener.take() {
            let spawned = std::thread::Builder::new()
                .name("ctdozlite-instance".into())
                .spawn(move || serve(listener, on_show));
            if let Err(err) = spawned {
                crate::logger::log_error("single_instance::spawn_show_listener", &err.to_string());
            }
        }
        #[cfg(not(unix))]
        let _ = on_show;
    }
}

#[cfg(unix)]
fn serve(listener: std::os::unix::net::UnixListener, on_show: impl Fn()) {
    use std::io::Read;
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        // A client that connects and never writes must not stall the loop.
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(1)));
        let mut buf = [0u8; 16];
        let mut len = 0;
        while len < buf.len() {
            match stream.read(&mut buf[len..]) {
                Ok(0) | Err(_) => break,
                Ok(n) => len += n,
            }
            if buf[..len].ends_with(b"\n") {
                break;
            }
        }
        if &buf[..len] == SHOW_REQUEST {
            on_show();
        }
    }
}

/// Bring the GUI window back from the tray, a minimized state, or behind other windows.
pub fn show_window(ctx: &eframe::egui::Context) {
    use eframe::egui::ViewportCommand;
    ctx.send_viewport_cmd(ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(ViewportCommand::Focus);
    ctx.request_repaint();
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    /// Fresh empty folder under the system temp dir; short enough for a Unix socket path.
    fn temp_dir(tag: &str) -> std::path::PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "ctdz-si-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn second_acquire_is_refused_when_first_is_held() {
        let dir = temp_dir("held");
        let first = acquire(&dir).unwrap();
        assert!(matches!(first, Acquired::Primary(_)));
        // flock is per open file description, so a second open in this process conflicts
        // exactly like another process would.
        assert!(matches!(acquire(&dir).unwrap(), Acquired::AlreadyRunning));
        drop(first);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn acquire_succeeds_when_lock_and_socket_files_are_stale() {
        let dir = temp_dir("stale");
        // Leftovers of a crashed instance: the files exist, but nobody holds the lock.
        std::fs::write(dir.join(LOCK_FILE), b"").unwrap();
        std::fs::write(dir.join(SOCKET_FILE), b"").unwrap();
        let guard = acquire(&dir).unwrap();
        let Acquired::Primary(guard) = guard else {
            panic!("stale files must not block a new instance");
        };
        assert!(
            guard.listener.is_some(),
            "stale socket file must be replaced"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn acquire_succeeds_when_previous_holder_released() {
        let dir = temp_dir("released");
        let first = acquire(&dir).unwrap();
        drop(first); // Closing the descriptor is what the kernel does when the holder dies.
        assert!(matches!(acquire(&dir).unwrap(), Acquired::Primary(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn notify_running_triggers_show_when_primary_listens() {
        let dir = temp_dir("notify");
        let Acquired::Primary(mut guard) = acquire(&dir).unwrap() else {
            panic!("first acquire must be primary");
        };
        let (tx, rx) = mpsc::channel();
        guard.spawn_show_listener(move || {
            let _ = tx.send(());
        });
        assert!(matches!(acquire(&dir).unwrap(), Acquired::AlreadyRunning));
        notify_running(&dir).unwrap();
        rx.recv_timeout(Duration::from_secs(5))
            .expect("running instance must receive the show request");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn notify_running_fails_when_nobody_listens() {
        let dir = temp_dir("silent");
        assert!(notify_running(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
