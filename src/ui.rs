//! Terminal I/O: credential prompts + a repainted chat list.
//!
//! Deliberately dependency-free (no ratatui): ANSI + Win32 console mode is all it needs.

use std::io::{self, BufRead, Write};

use anyhow::Result;

use crate::chat::Thread;

pub enum Action {
    None,
    Up,
    Down,
    Refresh,
    Quit,
}

pub fn prompt(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush()?;
    let mut s = String::new();
    io::stdin().lock().read_line(&mut s)?;
    Ok(s.trim_end_matches(['\r', '\n']).to_string())
}

pub fn prompt_default(label: &str, default: &str) -> Result<String> {
    let v = prompt(&format!("{label} [{default}]: "))?;
    Ok(if v.trim().is_empty() {
        default.to_string()
    } else {
        v
    })
}

/// Password prompt that does not echo what is typed.
///
/// Older builds read the password with `read_line`, which printed it on screen (toggling
/// `ENABLE_ECHO_INPUT` around `std::io::stdin()` deadlocks on Windows). This reads key events
/// through crossterm's raw mode instead, which goes through the console input API rather than
/// the std line buffer. Raw mode is restored on every exit path, including Ctrl+C / Esc.
/// When stdin is not a terminal (piped), one line is read instead — nothing to echo there.
pub fn prompt_password(label: &str) -> Result<String> {
    use std::io::IsTerminal;
    print!("{label}");
    io::stdout().flush()?;
    if !io::stdin().is_terminal() {
        return read_stdin_line();
    }
    let result = read_password_raw();
    // Raw mode is off again here (guard dropped inside), so a plain newline works.
    println!();
    match result? {
        Some(pwd) => Ok(pwd),
        None => anyhow::bail!("Đã huỷ nhập mật khẩu"),
    }
}

/// Collect a password from raw key events. `Ok(None)` = cancelled with Ctrl+C / Esc.
fn read_password_raw() -> Result<Option<String>> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    use crossterm::terminal;

    struct RawModeGuard;
    impl Drop for RawModeGuard {
        fn drop(&mut self) {
            let _ = terminal::disable_raw_mode();
        }
    }

    terminal::enable_raw_mode()?;
    let _guard = RawModeGuard;
    let mut pwd = String::new();
    loop {
        match event::read()? {
            // Windows reports both press and release; only act on presses / repeats.
            Event::Key(k) if k.kind != KeyEventKind::Release => match k.code {
                KeyCode::Enter => return Ok(Some(pwd)),
                KeyCode::Esc => return Ok(None),
                KeyCode::Char('c') | KeyCode::Char('C')
                    if k.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    return Ok(None)
                }
                KeyCode::Backspace => {
                    pwd.pop();
                }
                KeyCode::Char(c) => pwd.push(c),
                _ => {}
            },
            Event::Paste(s) => pwd.push_str(s.trim_end_matches(['\r', '\n'])),
            _ => {}
        }
    }
}

/// Read one line from stdin without a prompt (`--pass-stdin`).
pub fn read_stdin_line() -> Result<String> {
    let mut s = String::new();
    io::stdin().lock().read_line(&mut s)?;
    Ok(s.trim_end_matches(['\r', '\n']).to_string())
}

/// Current local time `HH:MM:SS` (operating-system time zone; was UTC before — issue #4).
pub fn clock() -> String {
    crate::timefmt::hms_now()
}

/// Repaint the thread list and wait for one key.
pub fn draw(threads: &[Thread], selected: usize, status: &str) -> Result<Action> {
    let mut out = io::stdout();
    write!(out, "\x1b[H\x1b[J")?;

    writeln!(
        out,
        "\x1b[1mZalo\x1b[0m  {} hội thoại  ·  {status}",
        threads.len()
    )?;
    writeln!(out, "{}", "─".repeat(64))?;

    if threads.is_empty() {
        writeln!(out, "  (chưa có hội thoại)")?;
    }

    for (i, t) in threads.iter().take(25).enumerate() {
        let marker = if i == selected { "▸" } else { " " };
        let name = fit(&t.name, 24);
        let prev = fit(&t.preview, 32);
        if i == selected {
            writeln!(out, "\x1b[7m{marker} {name}  {prev}\x1b[0m")?;
        } else {
            writeln!(out, "{marker} {name}  \x1b[90m{prev}\x1b[0m")?;
        }
    }

    writeln!(out, "{}", "─".repeat(64))?;
    writeln!(out, "\x1b[90m↑/↓ chọn (mỗi dòng một phím) · r làm mới · q thoát\x1b[0m")?;
    out.flush()?;

    // Line-buffered read: no terminal mode juggling, so it can never hang the console.
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line).is_err() {
        return Ok(Action::None);
    }
    let cmd = line.trim();

    Ok(match cmd {
        "q" | "Q" => Action::Quit,
        "r" | "R" => Action::Refresh,
        "k" => Action::Up,
        "j" => Action::Down,
        _ => Action::None,
    })
}

fn fit(s: &str, w: usize) -> String {
    let mut out = String::with_capacity(w);
    for ch in s.chars() {
        if out.chars().count() >= w {
            break;
        }
        out.push(if ch == '\n' || ch == '\t' { ' ' } else { ch });
    }
    let pad = w.saturating_sub(out.chars().count());
    out.extend(std::iter::repeat_n(' ', pad));
    out
}
