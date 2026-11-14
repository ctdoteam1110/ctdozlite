//! Complete native desktop GUI powered by `eframe` and `egui`.
//!
//! Designed with the UI/UX Pro Max Minimalism & Swiss Style system:
//! - Zero-idle-CPU reactive architecture (event-driven repaint, zero per-frame Thread cloning)
//! - Custom vector icons rendered via `egui::Painter` (zero texture/SVG bloat)
//! - High-contrast Light & Dark semantic color tokens (`#0068FF` / `#2563EB` primary, `#059669` online accent)
//! - Filterable conversation sidebar (`Tất cả`, `Cá nhân`, `Nhóm`), avatar badges, and grouped chat bubbles
//! - Built-in Zalo Sticker rendering & picker (`[Nhãn dán]`)

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use eframe::egui::{
    self, Align, Color32, CursorIcon, FontId, Id, Key, Layout, Margin, Pos2, Rect, RichText,
    Rounding, Sense, Stroke, TextureHandle, TextureOptions, Vec2,
};

use crate::chat::{ChatMessage, Thread};
use crate::logger::{self, FindingLevel, LogEntry, ScanFinding};
use crate::qr::QrLogin;
use crate::session::Session;

pub(crate) mod messages;
mod modals;
mod reactions_dialog;
mod stickers;
mod theme;
mod worker;
mod workspace;

use messages::*;
use stickers::*;
use theme::*;
pub use theme::{emoji_to_tex_key, load_app_icon, lucide, REACTION_LIST};
pub use worker::WorkerCmd;
use worker::*;

fn merge_sent_message(thread: &mut Thread, message: ChatMessage) {
    crate::chat::merge_sent_message(thread, message);
}

#[cfg_attr(not(any(target_os = "windows", test)), allow(dead_code))]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct WinRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl WinRect {
    #[cfg_attr(not(any(target_os = "windows", test)), allow(dead_code))]
    fn width(self) -> i32 {
        self.right.saturating_sub(self.left)
    }

    #[cfg_attr(not(any(target_os = "windows", test)), allow(dead_code))]
    fn height(self) -> i32 {
        self.bottom.saturating_sub(self.top)
    }
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug)]
struct WinResizeDrag {
    dir: egui::ResizeDirection,
    start_cursor_x: i32,
    start_cursor_y: i32,
    start_rect: WinRect,
    last_rect: WinRect,
}

/// Computes the target Win32 window rectangle during an 8-direction border drag,
/// clamping width and height to `(min_w, min_h)` without flipping opposite edges.
#[cfg_attr(not(any(target_os = "windows", test)), allow(dead_code))]
fn compute_resized_rect(
    start: WinRect,
    dir: egui::ResizeDirection,
    dx: i32,
    dy: i32,
    min_w: i32,
    min_h: i32,
) -> WinRect {
    let min_w = min_w.max(1);
    let min_h = min_h.max(1);
    let mut r = start;

    match dir {
        egui::ResizeDirection::West
        | egui::ResizeDirection::NorthWest
        | egui::ResizeDirection::SouthWest => {
            let max_left = r.right.saturating_sub(min_w);
            r.left = start.left.saturating_add(dx).min(max_left);
        }
        egui::ResizeDirection::East
        | egui::ResizeDirection::NorthEast
        | egui::ResizeDirection::SouthEast => {
            let min_right = r.left.saturating_add(min_w);
            r.right = start.right.saturating_add(dx).max(min_right);
        }
        egui::ResizeDirection::North | egui::ResizeDirection::South => {}
    }

    match dir {
        egui::ResizeDirection::North
        | egui::ResizeDirection::NorthWest
        | egui::ResizeDirection::NorthEast => {
            let max_top = r.bottom.saturating_sub(min_h);
            r.top = start.top.saturating_add(dy).min(max_top);
        }
        egui::ResizeDirection::South
        | egui::ResizeDirection::SouthWest
        | egui::ResizeDirection::SouthEast => {
            let min_bottom = r.top.saturating_add(min_h);
            r.bottom = start.bottom.saturating_add(dy).max(min_bottom);
        }
        egui::ResizeDirection::West | egui::ResizeDirection::East => {}
    }

    r
}

#[cfg(target_os = "windows")]
mod win32_frameless {
    use super::WinRect;
    use std::sync::{Mutex, OnceLock};

    const GWL_STYLE: i32 = -16;
    const GWL_EXSTYLE: i32 = -20;
    const GWLP_WNDPROC: i32 = -4;

    const WM_STYLECHANGING: u32 = 0x007C;
    const WM_NCDESTROY: u32 = 0x0082;

    // winit 0.30 keeps WS_CAPTION | WS_THICKFRAME and WS_EX_WINDOWEDGE even for undecorated
    // windows. Intel/hybrid WGL drivers use AdjustWindowRectEx on those bits and shift rendering,
    // leaving an unrendered black border on the top/left edges.
    const WS_BORDER: u32 = 0x0080_0000;
    const WS_DLGFRAME: u32 = 0x0040_0000;
    const WS_CAPTION: u32 = WS_BORDER | WS_DLGFRAME;
    const WS_THICKFRAME: u32 = 0x0004_0000;
    const STRIP_STYLE_MASK: u32 = WS_CAPTION | WS_THICKFRAME;

    const WS_EX_DLGMODALFRAME: u32 = 0x0000_0001;
    const WS_EX_WINDOWEDGE: u32 = 0x0000_0100;
    const WS_EX_CLIENTEDGE: u32 = 0x0000_0200;
    const STRIP_EX_STYLE_MASK: u32 = WS_EX_DLGMODALFRAME | WS_EX_WINDOWEDGE | WS_EX_CLIENTEDGE;

    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_NOACTIVATE: u32 = 0x0010;
    const SWP_FRAMECHANGED: u32 = 0x0020;

    const GW_OWNER: u32 = 4;
    const MONITOR_DEFAULTTONEAREST: u32 = 2;
    const SPI_GETWORKAREA: u32 = 0x0030;

    static ORIG_WNDPROCS: OnceLock<Mutex<Vec<(usize, usize)>>> = OnceLock::new();

    #[repr(C)]
    struct StyleStruct {
        style_old: u32,
        style_new: u32,
    }

    #[repr(C)]
    struct RawPoint {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    struct MonitorInfo {
        cb_size: u32,
        rc_monitor: WinRect,
        rc_work: WinRect,
        dw_flags: u32,
    }

    struct EnumSearchState {
        visible_match: usize,
        any_match: usize,
    }

    struct EnumTitleState<'a> {
        target_title: &'a str,
        matched: usize,
    }

    #[link(name = "opengl32")]
    extern "system" {
        fn wglGetCurrentDC() -> usize;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentThreadId() -> u32;
    }

    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: usize,
            dw_attribute: u32,
            pv_attribute: *const u32,
            cb_attribute: u32,
        ) -> i32;
    }

    #[link(name = "user32")]
    extern "system" {
        fn WindowFromDC(hdc: usize) -> usize;
        fn GetActiveWindow() -> usize;
        fn EnumThreadWindows(
            dw_thread_id: u32,
            lpfn: unsafe extern "system" fn(usize, isize) -> i32,
            l_param: isize,
        ) -> i32;
        fn GetWindow(hwnd: usize, u_cmd: u32) -> usize;
        fn GetWindowTextW(hwnd: usize, lp_string: *mut u16, n_max_count: i32) -> i32;
        fn IsWindow(hwnd: usize) -> i32;
        fn IsWindowVisible(hwnd: usize) -> i32;
        fn GetWindowLongW(hwnd: usize, n_index: i32) -> i32;
        fn SetWindowLongW(hwnd: usize, n_index: i32, dw_new_long: i32) -> i32;
        #[cfg(target_pointer_width = "64")]
        fn SetWindowLongPtrW(hwnd: usize, n_index: i32, dw_new_long: isize) -> isize;
        fn CallWindowProcW(
            lp_prev_wnd_func: usize,
            hwnd: usize,
            msg: u32,
            w_param: usize,
            l_param: isize,
        ) -> isize;
        fn SetWindowPos(
            hwnd: usize,
            hwnd_insert_after: usize,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            u_flags: u32,
        ) -> i32;
        fn GetWindowRect(hwnd: usize, lp_rect: *mut WinRect) -> i32;
        fn GetCursorPos(lp_point: *mut RawPoint) -> i32;
        fn MonitorFromWindow(hwnd: usize, dw_flags: u32) -> usize;
        fn GetMonitorInfoW(h_monitor: usize, lpmi: *mut MonitorInfo) -> i32;
        fn SystemParametersInfoW(
            ui_action: u32,
            ui_param: u32,
            pv_param: *mut WinRect,
            f_win_ini: u32,
        ) -> i32;
    }

    #[inline]
    fn strip_style_bits(style: u32) -> u32 {
        style & !STRIP_STYLE_MASK
    }

    #[inline]
    fn strip_ex_style_bits(ex_style: u32) -> u32 {
        ex_style & !STRIP_EX_STYLE_MASK
    }

    unsafe fn set_window_long_ptr(hwnd: usize, index: i32, new_val: isize) -> isize {
        #[cfg(target_pointer_width = "64")]
        // SAFETY: Caller passes a valid Win32 `HWND` and pointer-sized index/value on 64-bit Windows.
        unsafe {
            SetWindowLongPtrW(hwnd, index, new_val)
        }
        #[cfg(not(target_pointer_width = "64"))]
        // SAFETY: On 32-bit Windows `SetWindowLongW` accepts pointer-width values.
        unsafe {
            SetWindowLongW(hwnd, index, new_val as i32) as isize
        }
    }

    fn wndproc_map() -> &'static Mutex<Vec<(usize, usize)>> {
        ORIG_WNDPROCS.get_or_init(|| Mutex::new(Vec::new()))
    }

    fn lookup_orig_wndproc(hwnd: usize) -> usize {
        let guard = wndproc_map().lock().unwrap_or_else(|e| e.into_inner());
        guard
            .iter()
            .find(|&&(h, _)| h == hwnd)
            .map_or(0, |&(_, proc_ptr)| proc_ptr)
    }

    fn store_orig_wndproc(hwnd: usize, orig: usize) {
        let mut guard = wndproc_map().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = guard.iter_mut().find(|entry| entry.0 == hwnd) {
            entry.1 = orig;
        } else {
            guard.push((hwnd, orig));
        }
    }

    fn remove_orig_wndproc(hwnd: usize) -> usize {
        let mut guard = wndproc_map().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(idx) = guard.iter().position(|&(h, _)| h == hwnd) {
            guard.swap_remove(idx).1
        } else {
            0
        }
    }

    /// Intercepts `WM_STYLECHANGING` synchronously before Win32 commits `SetWindowLongW` changes,
    /// preventing `winit` (`set_visible`, maximize/restore, DPI updates) from re-enabling borders.
    unsafe extern "system" fn frameless_subclass_wndproc(
        hwnd: usize,
        msg: u32,
        w_param: usize,
        l_param: isize,
    ) -> isize {
        if msg == WM_STYLECHANGING && l_param != 0 {
            // Win32 places `GWL_STYLE` (-16) / `GWL_EXSTYLE` (-20) in the low 32 bits of `w_param`.
            let style_kind = w_param as i32;
            // SAFETY: For `WM_STYLECHANGING` with non-null `l_param`, Win32 guarantees `l_param`
            // points to a live mutable `STYLESTRUCT` for the duration of the message.
            let style_struct = unsafe { &mut *(l_param as *mut StyleStruct) };
            if style_kind == GWL_STYLE {
                style_struct.style_new = strip_style_bits(style_struct.style_new);
            } else if style_kind == GWL_EXSTYLE {
                style_struct.style_new = strip_ex_style_bits(style_struct.style_new);
            }
        }

        let orig = if msg == WM_NCDESTROY {
            let removed = remove_orig_wndproc(hwnd);
            if removed != 0 {
                // SAFETY: Restoring original `WndProc` on `WM_NCDESTROY` detaches subclass cleanly.
                let _ = unsafe { set_window_long_ptr(hwnd, GWLP_WNDPROC, removed as isize) };
            }
            removed
        } else {
            lookup_orig_wndproc(hwnd)
        };

        if orig != 0 {
            // SAFETY: `orig` is the valid previous `WndProc` saved when this subclass was installed.
            unsafe { CallWindowProcW(orig, hwnd, msg, w_param, l_param) }
        } else {
            0
        }
    }

    unsafe extern "system" fn enum_thread_window_cb(hwnd: usize, l_param: isize) -> i32 {
        // SAFETY: `l_param` points to stack-allocated `EnumSearchState` owned by `find_window_hwnd`.
        let state = unsafe { &mut *(l_param as *mut EnumSearchState) };
        // SAFETY: Win32 passes a valid thread `HWND` into `EnumThreadWindows` callback.
        let is_top_level = unsafe { GetWindow(hwnd, GW_OWNER) } == 0;
        if !is_top_level {
            return 1;
        }
        let Some(rect) = get_window_rect(hwnd) else {
            return 1;
        };
        // Skip winit/IME helper windows and the 380x156 corner popup so only the main window matches.
        if rect.width() < 400 || rect.height() < 300 {
            return 1;
        }
        // SAFETY: `hwnd` was enumerated by Win32 and verified non-null above.
        if unsafe { IsWindowVisible(hwnd) } != 0 {
            state.visible_match = hwnd;
            return 0;
        }
        if state.any_match == 0 {
            state.any_match = hwnd;
        }
        1
    }

    unsafe extern "system" fn enum_title_window_cb(hwnd: usize, l_param: isize) -> i32 {
        // SAFETY: `l_param` points to stack-allocated `EnumTitleState` owned by caller.
        let state = unsafe { &mut *(l_param as *mut EnumTitleState<'_>) };
        let mut buf = [0u16; 128];
        // SAFETY: `hwnd` comes from `EnumThreadWindows` and `buf` is a 128-unit stack buffer.
        let len = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
        if len > 0 {
            let title = String::from_utf16_lossy(&buf[..(len as usize).min(buf.len())]);
            if title == state.target_title {
                state.matched = hwnd;
                return 0;
            }
        }
        1
    }

    fn find_thread_window_by_title(title: &str) -> usize {
        let mut state = EnumTitleState {
            target_title: title,
            matched: 0,
        };
        // SAFETY: `state` outlives the synchronous `EnumThreadWindows` call on this thread.
        unsafe {
            let tid = GetCurrentThreadId();
            let _ = EnumThreadWindows(
                tid,
                enum_title_window_cb,
                (&mut state as *mut EnumTitleState<'_>) as isize,
            );
        }
        state.matched
    }

    pub(super) fn is_valid_window(hwnd: usize) -> bool {
        if hwnd == 0 {
            return false;
        }
        // SAFETY: `IsWindow` safely validates arbitrary handle integers without dereferencing.
        unsafe { IsWindow(hwnd) != 0 }
    }

    pub(super) fn find_window_hwnd() -> usize {
        // SAFETY: `wglGetCurrentDC` and `WindowFromDC` operate on thread-local GL state.
        let dc_hwnd = unsafe {
            let hdc = wglGetCurrentDC();
            if hdc == 0 {
                0
            } else {
                WindowFromDC(hdc)
            }
        };
        if dc_hwnd != 0
            && get_window_rect(dc_hwnd).is_some_and(|r| r.width() >= 400 && r.height() >= 300)
        {
            return dc_hwnd;
        }

        // Prefer exact main window title lookup before falling back to active/size heuristics.
        let by_title = find_thread_window_by_title("ctdozlite");
        if by_title != 0 {
            return by_title;
        }

        // SAFETY: `GetActiveWindow` returns the calling thread's active window or zero.
        let active = unsafe { GetActiveWindow() };
        if active != 0 {
            // SAFETY: `active` is non-zero and owned by the calling thread.
            let is_top_level = unsafe { GetWindow(active, GW_OWNER) } == 0;
            if is_top_level
                && get_window_rect(active).is_some_and(|r| r.width() >= 400 && r.height() >= 300)
            {
                return active;
            }
        }

        let mut state = EnumSearchState {
            visible_match: 0,
            any_match: 0,
        };
        // SAFETY: `state` outlives the synchronous `EnumThreadWindows` call on this thread.
        unsafe {
            let tid = GetCurrentThreadId();
            let _ = EnumThreadWindows(
                tid,
                enum_thread_window_cb,
                (&mut state as *mut EnumSearchState) as isize,
            );
        }
        if state.visible_match != 0 {
            state.visible_match
        } else {
            state.any_match
        }
    }

    /// Strips any pre-existing caption/thickframe bits and issues `SWP_FRAMECHANGED` so Win32 and
    /// WGL immediately reset the drawable origin to `(0, 0)`.
    pub(super) fn sync_frameless_style(hwnd: usize) -> bool {
        if !is_valid_window(hwnd) {
            return false;
        }
        // SAFETY: `hwnd` was just verified by `IsWindow` on the GUI thread.
        unsafe {
            let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
            let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            if (style & STRIP_STYLE_MASK) == 0 && (ex_style & STRIP_EX_STYLE_MASK) == 0 {
                return false;
            }
            let new_style = strip_style_bits(style) as i32;
            let new_ex_style = strip_ex_style_bits(ex_style) as i32;
            let _ = SetWindowLongW(hwnd, GWL_STYLE, new_style);
            let _ = SetWindowLongW(hwnd, GWL_EXSTYLE, new_ex_style);
            let _ = SetWindowPos(
                hwnd,
                0,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
            true
        }
    }

    /// Attaches the `WM_STYLECHANGING` filter once per `HWND` and strips initial border bits.
    /// Returns `(newly_subclassed, style_changed)`.
    pub(super) fn install_frameless_subclass(hwnd: usize) -> (bool, bool) {
        if !is_valid_window(hwnd) {
            return (false, false);
        }
        let mut newly_subclassed = false;
        if lookup_orig_wndproc(hwnd) == 0 {
            let sub_ptr = frameless_subclass_wndproc as *const () as usize;
            // SAFETY: `hwnd` is a valid Win32 window created on this GUI thread.
            let prev =
                unsafe { set_window_long_ptr(hwnd, GWLP_WNDPROC, sub_ptr as isize) } as usize;
            if prev != 0 && prev != sub_ptr {
                store_orig_wndproc(hwnd, prev);
                newly_subclassed = true;
            }
        }
        let style_changed = sync_frameless_style(hwnd);
        (newly_subclassed, style_changed)
    }

    /// Protects secondary frameless viewports (such as the corner notification popup) identified by
    /// their exact title on the GUI thread, returning `true` when style/subclass was just applied.
    pub(super) fn sync_frameless_window_by_title(title: &str, rounded_corners: bool) -> bool {
        let matched = find_thread_window_by_title(title);
        if matched == 0 {
            return false;
        }
        let (newly_subclassed, style_changed) = install_frameless_subclass(matched);
        if rounded_corners && (newly_subclassed || style_changed) {
            // Enable hardware-clipped rounded corners on Windows 11; Windows 10 ignores safely.
            const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
            const DWMWCP_ROUND: u32 = 2;
            let pref = DWMWCP_ROUND;
            // SAFETY: `matched` is a valid HWND and `pref` is a stack 4-byte DWORD.
            let _ = unsafe {
                DwmSetWindowAttribute(
                    matched,
                    DWMWA_WINDOW_CORNER_PREFERENCE,
                    &pref,
                    std::mem::size_of::<u32>() as u32,
                )
            };
        }
        newly_subclassed || style_changed
    }

    pub(super) fn get_window_rect(hwnd: usize) -> Option<WinRect> {
        if !is_valid_window(hwnd) {
            return None;
        }
        let mut raw = WinRect::default();
        // SAFETY: `hwnd` is a valid Win32 window and `raw` is a properly aligned output struct.
        let ok = unsafe { GetWindowRect(hwnd, &mut raw) != 0 };
        ok.then_some(raw)
    }

    pub(super) fn set_window_rect(hwnd: usize, rect: WinRect, frame_changed: bool) {
        if !is_valid_window(hwnd) {
            return;
        }
        let mut flags = SWP_NOZORDER | SWP_NOACTIVATE;
        if frame_changed {
            flags |= SWP_FRAMECHANGED;
        }
        // SAFETY: `hwnd` was verified by `IsWindow` and width/height are clamped positive.
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                0,
                rect.left,
                rect.top,
                rect.width().max(1),
                rect.height().max(1),
                flags,
            );
        }
    }

    pub(super) fn get_cursor_pos() -> Option<(i32, i32)> {
        let mut pt = RawPoint { x: 0, y: 0 };
        // SAFETY: `pt` is a stack-allocated Win32 `POINT` initialized to zero.
        let ok = unsafe { GetCursorPos(&mut pt) != 0 };
        ok.then_some((pt.x, pt.y))
    }

    pub(super) fn get_monitor_work_area(hwnd: usize) -> Option<WinRect> {
        if !is_valid_window(hwnd) {
            return None;
        }
        // SAFETY: `MonitorFromWindow` accepts valid `HWND` and `MonitorInfo` sets `cb_size` first.
        unsafe {
            let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            if mon == 0 {
                return None;
            }
            let mut info = MonitorInfo {
                cb_size: std::mem::size_of::<MonitorInfo>() as u32,
                rc_monitor: WinRect::default(),
                rc_work: WinRect::default(),
                dw_flags: 0,
            };
            if GetMonitorInfoW(mon, &mut info) == 0 {
                return None;
            }
            (info.rc_work.width() > 0 && info.rc_work.height() > 0).then_some(info.rc_work)
        }
    }

    pub(super) fn active_window_work_area() -> Option<WinRect> {
        // SAFETY: `GetActiveWindow` returns the thread's active `HWND` or zero.
        let active = unsafe { GetActiveWindow() };
        if let Some(work) = get_monitor_work_area(active) {
            return Some(work);
        }
        let hwnd = find_window_hwnd();
        if let Some(work) = get_monitor_work_area(hwnd) {
            return Some(work);
        }
        let mut primary_work = WinRect::default();
        // SAFETY: `primary_work` is a stack-allocated `RECT` passed to `SPI_GETWORKAREA`.
        let ok = unsafe {
            SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut primary_work as *mut WinRect, 0) != 0
        };
        (ok && primary_work.width() > 0 && primary_work.height() > 0).then_some(primary_work)
    }
}

/// Display name for a reaction sender inside a thread: group member name when
/// resolved, the sender id as-is when numeric/unknown, thread name otherwise.
fn resolve_reactor_name(thread: &Thread, sender_id: &str) -> String {
    if sender_id.trim().is_empty() {
        return thread.name.clone();
    }
    if let Some(member) = thread
        .presentation
        .members
        .iter()
        .find(|m| m.id == sender_id && !m.name.trim().is_empty())
    {
        return member.name.clone();
    }
    // 1-1 threads carry the peer name on the thread itself.
    if !thread.is_group && !thread.name.trim().is_empty() {
        return thread.name.clone();
    }
    sender_id.to_string()
}
/// Find the message a reaction targets: ID match first (`message_id`, then
/// `client_message_id`), timestamp only as fallback (#1745). Timestamp-first
/// could attribute a reaction to an outgoing message in an earlier thread
/// when two threads share a timestamp.
fn find_reaction_target<'a>(
    messages: &'a [ChatMessage],
    ev: &crate::reactions::ReactionEvent,
) -> Option<&'a ChatMessage> {
    if !ev.message_id.is_empty() {
        if let Some(m) = messages.iter().find(|m| m.id == ev.message_id) {
            return Some(m);
        }
    }
    if !ev.client_message_id.is_empty() {
        if let Some(m) = messages
            .iter()
            .find(|m| m.client_message_id == ev.client_message_id)
        {
            return Some(m);
        }
    }
    if ev.ts > 0 {
        return messages.iter().find(|m| m.ts == ev.ts);
    }
    None
}
pub enum GuiEvent {
    /// Terminal response for a Settings authorization action.
    NotificationAuthorization {
        request_id: u64,
        result: Result<
            crate::native_notifications::AuthorizationStatus,
            crate::native_notifications::NotificationError,
        >,
    },
    QrReady {
        qr_login: QrLogin,
        qr_image: egui::ColorImage,
    },
    QrScanned {
        name: String,
    },
    LoginSuccess {
        name: String,
        uin: String,
        avatar: Option<String>,
    },
    LoginFailed(String),
    ThreadsUpdated(Vec<Thread>),
    MessageSent {
        client_message_id: String,
        thread_id: String,
        text: String,
        msg_id: String,
    },
    StickerSent {
        client_message_id: String,
        thread_id: String,
        msg_id: String,
        sticker_id: u64,
        cate_id: u64,
        sticker_type: u32,
    },
    AttachmentSent {
        thread_id: String,
        msg: ChatMessage,
    },
    StickerImageReady {
        key: String,
        image: egui::ColorImage,
    },
    StickerImageFailed {
        key: String,
    },
    StickerStoreLoaded(Vec<crate::chat::StickerPackInfo>),
    StickerPackItemsLoaded {
        cate_id: u64,
        name: String,
        items: Vec<crate::chat::StickerItem>,
    },
    StickerSearchResults {
        keyword: String,
        items: Vec<crate::chat::StickerItem>,
    },
    StickerSuggestKeywordsLoaded(Vec<String>),
    ComposerStickerSuggestions {
        keyword: String,
        items: Vec<crate::chat::StickerItem>,
    },
    LinkPreviewReady(crate::chat::LinkMetadata),
    MessageTranslated {
        thread_id: String,
        msg_id: String,
        text: String,
        /// Generation lúc request được gửi; stale completion bị bỏ qua.
        generation: u64,
    },
    GroupMembersLoaded {
        group_id: String,
        members: Vec<crate::chat::SidebarMember>,
    },
    PhoneLookupSuccess(crate::chat::PhoneLookupResult),
    PhoneLookupFailed(String),
    FriendStatusChecked {
        user_id: String,
        status: crate::chat::FriendshipStatus,
        online: Option<(bool, String)>,
    },
    QuoteSent {
        client_message_id: String,
        thread_id: String,
        text: String,
        msg_id: String,
        quoted: ChatMessage,
    },
    Reacted {
        thread_id: String,
        msg_id: String,
        ts: i64,
        reaction: String,
    },
    ReactionEventsReceived(Vec<crate::reactions::ReactionEvent>),
    MessageDeleted {
        thread_id: String,
        msg_id: String,
        ts: i64,
    },
    MessageRecalled {
        thread_id: String,
        msg_id: String,
        ts: i64,
    },
    /// Sửa tin xong (WaveA3): tin cũ đã recall + tin mới đã gửi.
    /// GUI: thay text cũ = text mới, gắn `edited`, lưu bản cũ vào SQLite.
    MessageEdited {
        thread_id: String,
        old_msg_id: String,
        old_text: String,
        new_text: String,
        new_msg_id: String,
        new_ts: i64,
    },
    /// Tin nhắn tự xoá đã hết hạn (worker sweep 2s) — apply text biến mất vào GUI.
    VanishExpired {
        thread_id: String,
        expired_ids: Vec<String>,
    },
    /// Worker đã persist vanish xong — apply `VanishSetting` vào thread RAM.
    VanishSettingChanged {
        thread_id: String,
        duration_sec: i64,
        started_at_sec: i64,
    },
    ForwardFinished {
        target_name: String,
        error: Option<String>,
    },
    ScreenshotCaptured {
        path: std::path::PathBuf,
        width: u32,
        height: u32,
        image: egui::ColorImage,
    },
    TypingEventsReceived(Vec<crate::chat::TypingEvent>),
    SeenEventsReceived(Vec<crate::chat::SeenEvent>),
    SendFailed {
        thread_id: String,
        text: String,
        error: String,
    },
    SessionExpired(String),
    /// The server could not be reached: the session is intact, show it and retry (#21).
    SessionUnreachable(String),
    /// `zalo_history.db` + `threads_cache.json` were removed on request (#21).
    LocalDataWiped,
    CallStateChanged(String),
    IncomingCall {
        call_id: i64,
        caller_id: String,
        caller_name: String,
        is_video: bool,
        is_group: bool,
    },
    CallRejected {
        code: i64,
        reason: String,
    },
    StatusUpdate(String),
    RefreshFinished {
        threads: usize,
        messages: usize,
        errors: Vec<String>,
        manual: bool,
    },
    ScheduledListUpdated,
    SyncFinished {
        added: usize,
        errors: Vec<String>,
        cancelled: bool,
    },
    UpdateCheckFinished {
        result: Result<UpdateCheckResult, String>,
        silent: bool,
    },
    UpdateDownloadStatus {
        downloading: bool,
        message: String,
    },
    UpdateDownloadProgress {
        downloaded: u64,
        total: u64,
    },
    UpdateAppliedAndRestarting(String),
    SocialProfileLoaded(Result<crate::feed::SocialUserProfile, String>),
    SocialAvatarsLoaded(Result<Vec<crate::feed::FeedMedia>, String>),
    SocialTimelineLoaded(
        Result<
            (
                Vec<crate::feed::SocialUserProfile>,
                Vec<crate::feed::FeedItem>,
            ),
            String,
        >,
    ),
    MuteSnapshotSynced(HashMap<String, i64>),
    PinnedSnapshotSynced(Vec<String>),
    MuteEventsReceived(Vec<crate::chat::MuteControlEvent>),
    PhoneSyncProgress {
        progress: f32,
        status: String,
    },
    PhoneSyncFinished {
        threads: Vec<Thread>,
        added: usize,
        active_threads: usize,
        cancelled: bool,
        error: Option<String>,
    },
    /// Result of pinning/unpinning a single chat message through the group-board API.
    MessagePinResult {
        thread_id: String,
        msg_id: String,
        pin: bool,
        topic_id: String,
        error: Option<String>,
    },
    /// Server-side pinned board items for one conversation.
    BoardPinsUpdated {
        thread_id: String,
        topics: Vec<crate::chat::BoardTopic>,
        error: Option<String>,
    },
    ProxyUpdated(Result<Option<String>, String>),
    FileDownloaded(Result<(String, String), String>),
    /// Worker đã tải + trích text xem trước tài liệu (session-only).
    DocPreviewReady {
        msg_id: String,
        text: String,
    },
    /// Worker báo lỗi xem trước tài liệu (quá lớn / định dạng cũ / lỗi mạng).
    DocPreviewFailed {
        msg_id: String,
        err: String,
    },
    LoggedOut,
    HistoryReachedStart(String),
    AccountSwitched {
        uin: String,
        name: String,
        avatar: Option<String>,
        threads: Vec<Thread>,
    },
    /// Worker thread đã nạp xong note + watch của 1 nhóm (DB off UI thread).
    /// `note: None` = DB lỗi → disable editor, hiện retry.
    GroupNoteLoaded {
        thread_id: String,
        uin: String,
        note: Option<String>,
        watch: Vec<String>,
        error: Option<String>,
    },
    /// Kết quả auto-save note (debounce) từ worker thread.
    GroupNoteSaved {
        thread_id: String,
        saved_at_label: String,
        error: Option<String>,
    },
    /// Worker thread đã tính xong stats nhóm (DB + RAM snapshot).
    GroupStatsReady {
        thread_id: String,
        uin: String,
        stats: crate::group_stats::GroupStats,
    },
    /// 1..n reminder tới giờ (worker tick claim) → toast + click nhảy tới tin (WaveA1).
    ReminderDue(Vec<crate::store::ReminderItem>),
    /// Danh sách reminder thay đổi (thêm/xoá) → refresh modal (WaveA1).

    ReminderListUpdated,
    /// Script per-thread chạy xong → toast output (worker tick / on_message / chạy thử).
    ScriptToast { thread_id: String, message: String },
    /// Kết quả rời nhóm gửi từ worker thread
    GroupLeft {
        group_id: String,
        success: bool,
        error: Option<String>,
    },
    /// Log script của thread đang mở thay đổi → drawer Scripts refresh.
    ScriptLogsUpdated { thread_id: String },
    /// Người dùng đóng hộp thoại chọn tệp mà không chọn tệp nào -> khôi phục caption đang gõ.
    AttachmentPickerCancelled { thread_id: String, caption: String },
    /// Kết quả tóm tắt nhóm chạy off-UI-thread (`INV-3`).
    GroupSummaryReady { message: String },
    /// Danh sách script của thread đã nạp từ SQLite off-UI-thread (`INV-3`).
    ThreadScriptsLoaded {
        thread_id: String,
        scripts: Vec<crate::thread_script::ThreadScript>,
        logs: Vec<crate::thread_script::ScriptLog>,
    },
    /// Kết quả lưu / xoá / bật-tắt script trong SQLite off-UI-thread (`INV-3`).
    ThreadScriptSaved {
        thread_id: String,
        close_editor: bool,
        ok: bool,
        toast: Option<String>,
        logs: Option<Vec<crate::thread_script::ScriptLog>>,
    },
    /// Kết quả chạy thử script Rhai off-UI-thread (`INV-3`).
    ThreadScriptRunResult {
        thread_id: String,
        output: String,
        outbox: Vec<crate::thread_script::OutboxMsg>,
        toasts: Vec<String>,
        is_group: bool,
        logs: Vec<crate::thread_script::ScriptLog>,
    },
}

fn phone_sync_completion_message(
    added: usize,
    active_threads: usize,
    cancelled: bool,
    error: Option<&str>,
) -> String {
    if let Some(err) = error {
        return format!("⚠️ Đồng bộ thất bại: {err}");
    }
    let prefix = if cancelled {
        "Đã dừng"
    } else {
        "Đã hoàn tất"
    };
    format!(
        "{prefix} yêu cầu: {added} tin mới từ Cloud/Nhóm ({active_threads} hội thoại). Lịch sử điện thoại chưa được tải vào ứng dụng."
    )
}

#[derive(Debug, Clone)]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
    pub release_title: String,
    pub release_notes: String,
    pub html_url: String,
    pub download_url: Option<String>,
    pub asset_name: Option<String>,
    pub asset_size: u64,
    pub expected_sha256: Option<String>,
    pub published_at: String,
}

/// Compare semantic versions (`"v0.0.2"` > `"0.0.1"`).
pub(crate) fn default_quick_reply_items() -> Vec<String> {
    vec![
        "Anh/em đang bận chút, lát gọi lại nhé!".into(),
        "Đang trong cuộc họp, nhắn tin giúp mình nhé.".into(),
        "Hiện không tiện nghe máy, có việc gì gấp không ạ?".into(),
        "Đang ngoài đường ồn quá, lát về mình gọi lại nha.".into(),
    ]
}

/// Compare semantic versions (`"v0.0.2"` > `"0.0.1"`).
pub fn is_newer_semver(remote: &str, current: &str) -> bool {
    crate::updater::is_newer_semver(remote, current)
}

/// Query GitHub Releases API (`/repos/ctdoteam1110/ctdozlite/releases/latest`) for updates.
pub async fn check_github_release_cli() -> Result<UpdateCheckResult, String> {
    worker::fetch_github_latest_release().await
}

#[derive(Debug, Clone)]
struct ActiveCallOverlay {
    partner_id: String,
    partner_name: String,
    is_video: bool,
    is_group: bool,
    state_label: String,
    started_at: std::time::Instant,
    mic_muted: bool,
    cam_enabled: bool,
    is_incoming_ringing: bool,
    error_code: Option<i64>,
    error_reason: Option<String>,
    speaker_view: bool,
    pinned_tile_idx: usize,
    call_page: usize,
    members_popover_open: bool,
    invite_search_query: String,
    quick_reply_popover_open: bool,
}

#[derive(Debug, Clone)]
struct ImageViewerModal {
    tex_key: String,
    hd_tex_key: Option<String>,
    full_url: String,
    title: String,
    sender: String,
    time: String,
    zoom: f32,
    pan: Vec2,
}

#[derive(Debug, Clone)]
pub(crate) struct VideoPlayerModal {
    pub url: String,
    pub title: String,
    pub sender: String,
    pub time: String,
    pub thumb_key: Option<String>,
    pub is_downloaded: bool,
    pub is_downloading: bool,
    pub play_on_download: bool,
    pub cached_path: Option<std::path::PathBuf>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeedModalTab {
    Profile,
    Avatars,
    Packet50001,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContactsSubTab {
    Friends,
    Groups,
    FriendRequests,
    GroupInvites,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    Account,
    Privacy,
    Sync,
    Appearance,
    Notifications,
    Messages,
    Utilities,
    Update,
}
fn settings_default_true() -> bool {
    true
}

/// Rule lọc của 1 Chat Folder (WaveA3, Telegram-style).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct ChatFolder {
    /// Tên hiển thị (duy nhất, trim).
    pub name: String,
    /// Lọc theo nhãn phân loại (thread_labels). Rỗng = mọi nhãn.
    pub labels: Vec<String>,
    /// Chỉ thread 1-1 (cá nhân).
    pub only_direct: bool,
    /// Chỉ nhóm.
    pub only_groups: bool,
    /// Chỉ thread có tin chưa đọc.
    pub only_unread: bool,
    /// Gán tay theo thread id (chuột phải → thêm/bỏ).
    pub thread_ids: Vec<String>,
}

impl ChatFolder {
    /// `true` khi folder không có rule nào (hiện tất cả như tab Tất cả).
    pub fn is_empty_rule(&self) -> bool {
        self.labels.is_empty()
            && self.thread_ids.is_empty()
            && !self.only_direct
            && !self.only_groups
            && !self.only_unread
    }

    pub(crate) fn matches_label(
        &self,
        thread_id: &str,
        labels: &HashMap<String, String>,
    ) -> bool {
        if self.labels.is_empty() {
            return true;
        }
        labels
            .get(thread_id)
            .is_some_and(|l| self.labels.iter().any(|x| x == l))
    }
}

/// Tin đang sửa (WaveA3): editor mở với text cũ; Enter = recall cũ + gửi mới.
#[derive(Debug, Clone)]
pub(crate) struct EditingMessage {
    pub thread_id: String,
    pub is_group: bool,
    pub msg_id: String,
    pub msg_ts: i64,
    pub old_text: String,
    pub draft: String,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct AppSettings {
    // 1. Cài đặt chung
    show_all_contacts: bool,
    language: String,
    autostart_on_boot: bool,
    remember_login: bool,
    download_dir: String,
    /// Mã ngôn ngữ đích cho dịch tin nhắn (vi/en/zh/ja/ko...). Mặc định vi.
    translate_target: String,
    // 2. Quyền riêng tư
    show_birthday: String,
    show_online_status: bool,
    show_seen_status: bool,
    allow_message_from: String,
    block_typing: bool,
    block_delivered: bool,
    block_seen: bool,
    allow_call_from: String,
    allow_search_by_phone: bool,
    friend_src_qr: bool,
    friend_src_group: bool,
    friend_src_card: bool,
    // 3. Giao diện
    pub(crate) theme_mode: String, // "light" | "dark" | "system"
    pub(crate) use_avatar_as_wallpaper: bool,
    pub(crate) font_family: String,
    pub(crate) font_scale: f32,
    #[serde(default = "settings_default_true")]
    pub(crate) animations_enabled: bool,
    #[serde(default = "settings_default_animation_speed")]
    pub(crate) animation_speed: f32,
    pub(crate) custom_primary_color: Option<String>,
    pub(crate) custom_accent_color: Option<String>,
    pub(crate) custom_bubble_out_bg: Option<String>,
    pub(crate) custom_bubble_in_bg: Option<String>,
    // 4. Thông báo
    enable_notifications: bool,
    play_notification_sound: bool,
    // 5. Tin nhắn
    quick_messages_enabled: bool,
    quick_reply_enabled: bool,
    quick_reply_items: Vec<String>,
    #[serde(skip)]
    quick_reply_draft: String,
    hidden_chat_pin_enabled: bool,
    split_priority_inbox: bool,
    excel_paste_mode: String, // "image" | "text" | "ask"
    #[serde(default = "settings_default_true")]
    render_markdown: bool,
    #[serde(default = "settings_default_true")]
    zalo_single_star_bold: bool,
    double_click_to_reply: bool,
    show_typing_indicator: bool,
    send_message_key: String, // "enter" | "cmd_enter"
    // 6. Tiện ích
    suggest_stickers_while_typing: bool,
    suggest_mentions: bool,
    enable_gif_search: bool,
    enable_sticker_keyword_search: bool,
    // 7. Thiết bị cuộc gọi (Mic & Camera)
    selected_mic_device: String,
    selected_camera_device: String,
    mic_gain_percent: u32,
    // 8. Quản lý hội thoại (Context menu: Ghim, Phân loại, Tắt thông báo, Ẩn)
    pinned_threads: Vec<String>,
    muted_threads: HashMap<String, i64>,
    hidden_threads: HashSet<String>,
    // Nhãn phân loại thread (WaveA3 gán tay chuột phải). Sibling đang dùng nhưng thiếu khai báo.
    #[serde(default)]
    thread_labels: HashMap<String, String>,
    #[serde(default)]
    chat_folders: Vec<ChatFolder>,
    /// Folder đang active ("" = Tất cả). Lưu cùng settings.
    #[serde(default)]
    active_chat_folder: String,
    // 9. Cập nhật tự động qua GitHub Releases API
    auto_check_updates: bool,
    skipped_update_version: String,
    // 10. Proxy & Đa tài khoản (#64)
    proxy_url: String,
    multi_profile_name: String,
    info_drawer_open: bool,
}

impl Default for AppSettings {
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
            show_all_contacts: false,
            language: "Tiếng Việt".into(),
            autostart_on_boot: true,
            remember_login: true,
            download_dir: default_dl,
            translate_target: "vi".into(),
            show_birthday: "Không hiện".into(),
            show_online_status: true,
            show_seen_status: true,
            block_typing: false,
            block_delivered: false,
            block_seen: false,
            allow_message_from: "Tất cả mọi người".into(),
            allow_call_from: "Bạn bè và người lạ từng liên hệ".into(),
            allow_search_by_phone: true,
            friend_src_qr: true,
            friend_src_group: true,
            friend_src_card: true,
            theme_mode: "dark".into(),
            use_avatar_as_wallpaper: false,
            font_family: "default".into(),
            font_scale: 1.0,
            animations_enabled: true,
            animation_speed: 1.0,
            custom_primary_color: None,
            custom_accent_color: None,
            custom_bubble_out_bg: None,
            custom_bubble_in_bg: None,
            enable_notifications: true,
            play_notification_sound: true,
            quick_messages_enabled: true,
            quick_reply_enabled: true,
            quick_reply_items: default_quick_reply_items(),
            quick_reply_draft: String::new(),
            hidden_chat_pin_enabled: true,
            split_priority_inbox: false,
            excel_paste_mode: "ask".into(),
            render_markdown: true,
            zalo_single_star_bold: true,
            double_click_to_reply: true,
            show_typing_indicator: true,
            send_message_key: "enter".into(),
            suggest_stickers_while_typing: true,
            suggest_mentions: true,
            enable_gif_search: true,
            enable_sticker_keyword_search: true,
            selected_mic_device: String::new(),
            selected_camera_device: String::new(),
            mic_gain_percent: 100,
            pinned_threads: Vec::new(),
            muted_threads: HashMap::new(),
            hidden_threads: HashSet::new(),
            thread_labels: HashMap::new(),
            chat_folders: Vec::new(),
            active_chat_folder: String::new(),
            auto_check_updates: true,
            skipped_update_version: String::new(),
            proxy_url: String::new(),
            multi_profile_name: "acc2".into(),
            info_drawer_open: false,
        }
    }
}

impl AppSettings {
    pub(crate) fn normalized_font_scale(&self) -> f32 {
        if self.font_scale.is_nan() || self.font_scale.is_infinite() || self.font_scale <= 0.0 {
            1.0
        } else {
            self.font_scale.clamp(0.5, 3.0)
        }
    }

    pub(crate) fn normalized_animation_speed(&self) -> f32 {
        if !self.animation_speed.is_finite() || self.animation_speed <= 0.0 {
            1.0
        } else {
            self.animation_speed.clamp(0.5, 2.0)
        }
    }

    fn load() -> Self {
        std::fs::read_to_string("app_settings.json")
            .ok()
            .and_then(|s| serde_json::from_str::<Self>(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write("app_settings.json", json);
        }
    }
}

/// A loaded-message jump stays bound to its conversation and exact reference.
struct QuoteJump {
    thread_id: String,
    reference: String,
    highlight_until: std::time::Instant,
    scroll_pending: bool,
}

impl QuoteJump {
    fn target_index(&self, thread_id: &str, messages: &[ChatMessage]) -> Option<usize> {
        if self.thread_id != thread_id || std::time::Instant::now() >= self.highlight_until {
            return None;
        }
        crate::message_navigation::unique_reference_index(messages, &self.reference)
    }
}

/// Bubbles rendered per step in the chat view; "Tải thêm tin nhắn cũ" adds another step.
const CHAT_RENDER_STEP: usize = 150;

/// Worker → GUI channel that wakes the UI on every send. The GUI no longer repaints on a
/// 1.2 s timer, so an event sent without a repaint would sit unseen until the next mouse move.
#[derive(Clone)]
pub struct GuiTx {
    tx: Sender<GuiEvent>,
    ctx: egui::Context,
}

impl GuiTx {
    pub fn new(tx: Sender<GuiEvent>, ctx: egui::Context) -> Self {
        Self { tx, ctx }
    }

    /// Same contract as `Sender::send` (callers already `let _ =` it), plus the wake-up.
    #[allow(clippy::result_large_err)]
    pub fn send(&self, ev: GuiEvent) -> Result<(), std::sync::mpsc::SendError<GuiEvent>> {
        let res = self.tx.send(ev);
        self.ctx.request_repaint();
        res
    }
}

#[derive(Clone)]
pub(crate) struct IncomingPopupState {
    pub(crate) thread_id: String,
    pub(crate) sender_name: String,
    pub(crate) preview_text: String,
    pub(crate) avatar_url: Option<String>,
    pub(crate) is_group: bool,
    pub(crate) unread_total: usize,
    pub(crate) created_at: std::time::Instant,
    pub(crate) reply_draft: String,
}

pub struct ZaloGuiApp {
    // Theme state
    dark_mode: bool,
    theme_dirty: bool,
    cached_palette: ThemePalette,

    // Session state
    is_logged_in: bool,
    user_name: String,
    user_uin: String,
    user_avatar: Option<String>,
    /// UIN tài khoản đang dùng trước khi bấm "Thêm tài khoản mới" (#103):
    /// login screen hiện nút quay lại, Hủy → SwitchAccount về uin này.
    pub(crate) pending_add_account_uin: Option<String>,

    // Login screen state
    login_tab: LoginTab,
    phone_input: String,
    password_input: String,
    login_in_progress: bool,
    login_status_msg: String,

    // QR state
    qr_texture: Option<TextureHandle>,
    qr_status: String,

    // Chat screen state
    threads: Vec<Thread>,
    /// Event ledger for reaction counts (see `reactions::ReactionInbox`).
    reaction_inbox: crate::reactions::ReactionInbox,
    selected_thread_index: usize,
    thread_filter: ThreadFilter,
    search_query: String,
    input_message: String,
    status_msg: String,
    pub(crate) voice_recorder: Option<crate::audio::VoiceRecorder>,

    // Reply / forward state
    replying_to: Option<ChatMessage>,
    forward_msg: Option<ChatMessage>,
    forward_filter: String,
    /// (thread id, message id) whose reaction list is open.
    reactions_dialog: Option<(String, String)>,
    /// Selected reaction type tab in that dialog (None = all).
    reactions_tab: Option<i32>,
    filter_starred_only: bool,
    pub(crate) send_hd_original: bool,
    pub(crate) typing_map: HashMap<String, (String, std::time::Instant)>,
    active_call: Option<ActiveCallOverlay>,
    image_viewer: Option<ImageViewerModal>,
    pub(crate) video_player: Option<VideoPlayerModal>,

    info_drawer_open: bool,
    profile_menu_open: bool,
    /// "Xoá dữ liệu tin nhắn cục bộ" is pending confirmation (#21).
    wipe_confirm_open: bool,
    /// "Đăng xuất" is pending confirmation (#130).
    pub(crate) logout_confirm_open: bool,
    toast_notice: Option<(String, std::time::Instant)>,
    /// Rich corner notification popup for incoming messages
    incoming_popup: Option<IncomingPopupState>,
    phone_modal_open: bool,
    phone_modal_input: String,
    phone_lookup_loading: bool,
    phone_lookup_result: Option<crate::chat::PhoneLookupResult>,
    phone_lookup_error: Option<String>,
    in_chat_search_open: bool,
    in_chat_search_query: String,
    in_chat_search_focus_pending: bool,
    /// Picker chọn thời gian tin nhắn tự xoá ở thread header (local-only).
    vanish_picker_open: bool,
    // Sticker picker & image cache state
    sticker_picker_open: bool,
    sticker_tab: StickerTab,
    sticker_textures: HashMap<String, TextureHandle>,
    loading_stickers: HashSet<String>,
    /// Tracks sticker/avatar/thumb URLs that failed to download: `(attempts, last_failed_at)`.
    failed_stickers: HashMap<String, (u8, std::time::Instant)>,
    emoji_textures: HashMap<String, TextureHandle>,
    app_logo_texture: Option<TextureHandle>,
    sticker_store_packs: Vec<crate::chat::StickerPackInfo>,
    sticker_store_loading: bool,
    sticker_store_attempted: bool,
    sticker_store_filter: String,
    sticker_pack_items: HashMap<u64, Vec<crate::chat::StickerItem>>,
    loading_sticker_packs: HashSet<u64>,
    failed_sticker_packs: HashSet<u64>,
    pinned_sticker_packs: Vec<(u64, String)>,
    sticker_search_query: String,
    sticker_search_results: Vec<crate::chat::StickerItem>,
    sticker_search_loading: bool,
    last_sticker_search_dispatched: String,
    last_sticker_search_edit_at: std::time::Instant,
    sticker_suggest_keywords: HashSet<String>,
    composer_sticker_cache: HashMap<String, Vec<crate::chat::StickerItem>>,
    composer_sticker_loading: HashSet<String>,
    dismissed_sticker_suggest_for: String,
    last_composer_text: String,
    last_composer_edit_at: std::time::Instant,
    composer_sticker_selected: Option<usize>,
    composer_mention_selected: usize,
    dismissed_mention_suggest_for: String,
    composer_focus_pending: bool,
    pending_clipboard_image: Option<(std::path::PathBuf, u32, u32, TextureHandle)>,
    unread_counts: HashMap<String, usize>,
    reaction_unread_counts: HashMap<String, usize>,
    link_metadata_cache: HashMap<String, crate::chat::LinkMetadata>,
    loading_link_previews: HashSet<String>,
    /// Tin đang chờ dịch (translate_request_key) → disable menu Dịch.
    translate_pending: HashSet<String>,
    /// Generation mới nhất của mỗi request dịch → bỏ stale completion.
    translate_generation: HashMap<String, u64>,
    dismissed_link_preview_for: String,
    friend_status_map: HashMap<String, crate::chat::FriendshipStatus>,
    checking_friend_status: HashSet<String>,
    group_members_map: HashMap<String, Vec<crate::chat::SidebarMember>>,
    fetching_group_members: HashSet<String>,
    group_member_search: String,
    /// Group Workspace: ghi chú đang sửa theo thread (thread_id → text).
    group_note_drafts: HashMap<String, String>,
    /// Thread đã nạp ghi chú từ SQLite (tránh đọc lại mỗi frame).
    group_note_loaded: HashSet<String>,
    /// Lần sửa ghi chú cuối (thread_id → instant) để auto-save debounce ~1s.
    /// Giữ pending khi save fail → tick sau thử lại (không mất draft).
    group_note_edited_at: HashMap<String, std::time::Instant>,
    /// Mốc save fail gần nhất (thread_id → instant): debounce retry, tránh spam DB.
    group_note_save_retry_at: HashMap<String, std::time::Instant>,
    /// Trạng thái lưu ghi chú hiển thị dưới editor ("Đã lưu HH:MM" / "Đang lưu…").
    group_note_saved_label: HashMap<String, String>,
    /// Từ khóa mới đang nhập trong ô theo dõi (theo thread).
    group_watch_inputs: HashMap<String, String>,
    /// Cache keyword DB để render list (thread_id → keywords).
    group_watch_cache: HashMap<String, Vec<String>>,
    /// Stats nhóm đang hiển thị (thread_id → (fetched_at, stats)).
    group_stats_cache: HashMap<String, (std::time::Instant, crate::group_stats::GroupStats)>,
    /// Nhóm đang nạp note/watch async (tránh spawn trùng mỗi frame).
    group_note_loading: HashSet<String>,
    /// Nhóm nạp note/watch lỗi DB (thread_id → message): editor disable + nút thử lại.
    group_note_load_error: HashMap<String, String>,
    /// Nhóm đang tính stats async (tránh spawn trùng mỗi frame).
    group_stats_loading: HashSet<String>,
    /// Toast tóm tắt nhóm chờ hiển thị sau khi vẽ xong workspace (tránh borrow ui).
    group_summary_pending: Option<String>,
    /// Nhảy tới tin khớp chờ thực hiện sau khi vẽ xong workspace.
    group_jump_pending: Option<(String, String)>,
    /// Scripts per-thread: tab drawer (0 = list, 1 = editor), script đang sửa, draft code.
    script_tab: usize,
    script_list_cache: HashMap<String, Vec<crate::thread_script::ThreadScript>>,
    script_logs_cache: HashMap<String, Vec<crate::thread_script::ScriptLog>>,
    script_loading_threads: HashSet<String>,
    script_editing_id: Option<i64>,
    script_name_draft: String,
    script_trigger_draft: String,
    script_arg_draft: String,
    script_code_draft: String,
    script_output: String,
    /// Pending confirmation before permanently deleting a thread (`(idx, tid, name, is_group, last_msg_id)`).
    pending_delete_thread: Option<(usize, String, String, bool, String)>,
    /// Pending confirmation before leaving a group (`(idx, group_id, group_name)`).
    pub(crate) pending_leave_group: Option<(usize, String, String)>,
    /// Xem trước tài liệu (session-only): msg_id → text đã trích.
    doc_previews: HashMap<String, String>,
    /// msg_id đang tải xem trước (tránh gửi trùng mỗi frame).
    doc_preview_loading: HashSet<String>,
    /// msg_id xem trước lỗi (msg_id → message): hiện lỗi + cho thử lại.
    doc_preview_errors: HashMap<String, String>,
    /// Popup xem đầy đủ text tài liệu (msg_id, file_name, text).
    doc_viewer: Option<(String, String, String)>,
    user_online_map: HashMap<String, (bool, String)>,
    allow_quit: bool,
    settings_modal_open: bool,
    settings_tab: SettingsTab,
    notification_authorization: crate::native_notifications::AuthorizationState,
    settings: AppSettings,
    settings_blur_tex: Option<TextureHandle>,
    settings_blur_req_at: Option<std::time::Instant>,
    update_checking: bool,
    update_downloading: bool,
    update_progress: Option<(u64, u64)>,
    update_status: String,
    update_info: Option<UpdateCheckResult>,
    update_modal_open: bool,
    update_banner_dismissed: bool,
    last_notified_update_version: Option<String>,
    dev_mode: bool,
    dev_panel_open: bool,
    dev_tab: DevTab,
    selected_log: Option<LogEntry>,
    scanner_findings: Vec<ScanFinding>,
    feed_modal_open: bool,
    feed_modal_tab: FeedModalTab,
    feed_target_uid: String,
    feed_loading_profile: bool,
    feed_profile: Option<crate::feed::SocialUserProfile>,
    feed_profile_error: Option<String>,
    feed_loading_avatars: bool,
    feed_avatars: Vec<crate::feed::FeedMedia>,
    feed_avatars_error: Option<String>,
    feed_loading_timeline: bool,
    feed_items: Vec<crate::feed::FeedItem>,
    feed_friend_profiles: Vec<crate::feed::SocialUserProfile>,
    feed_search_query: String,
    feed_pkt_owner_id: String,
    feed_pkt_viewer_id: String,
    feed_pkt_tab: u32,
    feed_pkt_page: u32,
    feed_pkt_count: u32,
    contacts_view_open: bool,
    contacts_sub_tab: ContactsSubTab,
    contacts_search_query: String,
    schedule_modal_open: bool,
    schedule_custom_input: String,
    schedule_editing_id: Option<i64>,
    scheduled_items: Vec<crate::scheduler::ScheduledMessage>,
    phone_sync_progress: f32,
    phone_sync_status: String,
    /// Hộp "Đã lưu" toàn cục (WaveA2): modal + cache + draft ghi chú/hạn.
    pub(crate) saved_modal_open: bool,
    pub(crate) saved_items: Vec<crate::store::SavedItem>,
    pub(crate) saved_note_drafts: std::collections::HashMap<String, String>,
    pub(crate) saved_due_drafts: std::collections::HashMap<String, String>,
    /// Nháp composer theo hội thoại (WaveA1): thread_id → text. Persist SQLite bảng `drafts`.
    draft_map: HashMap<String, String>,
    /// reply_to đang treo theo hội thoại (WaveA1): thread_id → msg_id được trả lời.
    draft_reply_map: HashMap<String, String>,
    /// Thread đang có nháp (cache từ `list_draft_threads`, để vẽ badge "nháp") (WaveA1).
    draft_threads: HashSet<String>,
    /// Modal quản lý reminder + cache list (WaveA1).
    reminder_modal_open: bool,
    reminder_items: Vec<crate::store::ReminderItem>,
    /// Reminder tới giờ chờ click nhảy tới tin (toast click → QuoteJump) (WaveA1).
    reminder_due_pending: Vec<crate::store::ReminderItem>,
    /// Tin đang chọn nhắc (picker nhanh: thread_id, msg_id, preview) (WaveA1).
    remind_pick_msg: Option<(String, String, String)>,
    remind_custom_open: bool,
    remind_custom_input: String,
    /// Tin đang sửa (WaveA3): None = không sửa; Some = editor mở với text cũ.
    pub(crate) editing_message: Option<EditingMessage>,
    /// Lịch sử bản cũ đã nạp (msg_id → records) để vẽ modal (WaveA3).
    pub(crate) edit_history_cache: HashMap<String, Vec<crate::store::MessageEditRecord>>,
    /// Modal lịch sử sửa đang mở (msg_id, text hiện tại) (WaveA3).
    pub(crate) edit_history_open: Option<(String, String)>,
    /// Modal quản lý Chat Folders (WaveA3).
    pub(crate) folder_modal_open: bool,
    last_typing_sent_at: Option<(String, std::time::Instant)>,
    last_synced_icon_unread: Option<usize>,
    // Background channels
    event_rx: Receiver<GuiEvent>,
    #[allow(dead_code)]
    event_tx: Sender<GuiEvent>,
    /// (thread id, number of newest bubbles rendered) — see `CHAT_RENDER_STEP`.
    chat_render_window: (String, usize),
    quote_jump: Option<QuoteJump>,
    ime_active: bool,
    ime_just_committed: bool,
    history_reached_start: HashSet<String>,
    force_scroll_to_bottom: bool,
    #[cfg(target_os = "windows")]
    win_hwnd: usize,
    #[cfg(target_os = "windows")]
    win_maximized: bool,
    #[cfg(target_os = "windows")]
    win_restore_rect: Option<WinRect>,
    #[cfg(target_os = "windows")]
    win_resize_drag: Option<WinResizeDrag>,
}

#[derive(Debug, PartialEq, Eq)]
enum LoginTab {
    QrCode,
    Password,
}

#[derive(Debug, PartialEq, Eq)]
enum DevTab {
    Logs,
    Scanner,
    CryptoState,
}

/// Every user-visible string in `threads` (names + message bodies), for font coverage checks.
fn thread_texts(threads: &[Thread]) -> impl Iterator<Item = &str> {
    threads.iter().flat_map(|t| {
        std::iter::once(t.name.as_str()).chain(t.messages.iter().map(|m| m.text.as_str()))
    })
}

impl ZaloGuiApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        mut session: Session,
        dev_mode: bool,
        initial_threads: Vec<Thread>,
    ) -> Self {
        let mut loaded_settings = AppSettings::load();
        setup_custom_fonts_with_choice(&cc.egui_ctx, &loaded_settings.font_family);
        let safe_scale = loaded_settings.normalized_font_scale();
        cc.egui_ctx.set_zoom_factor(safe_scale);
        if session.proxy_url.is_none() && !loaded_settings.proxy_url.trim().is_empty() {
            let _ = session.set_proxy(Some(&loaded_settings.proxy_url));
        } else if let Some(ref p) = session.proxy_url {
            if loaded_settings.proxy_url.trim().is_empty() {
                loaded_settings.proxy_url = p.clone();
            }
        }
        crate::media::set_selected_mic_device(&loaded_settings.selected_mic_device);
        crate::media::set_selected_camera_device(&loaded_settings.selected_camera_device);
        crate::media::set_mic_gain_percent(loaded_settings.mic_gain_percent);
        crate::session::set_privacy_blocker(
            loaded_settings.block_typing,
            loaded_settings.block_delivered,
            loaded_settings.block_seen,
        );
        let dark_mode = resolve_dark_mode(&loaded_settings.theme_mode, &cc.egui_ctx);
        let mut initial_palette = ThemePalette::load_with_custom_override(dark_mode);
        initial_palette.apply_custom_colors(
            loaded_settings.custom_primary_color.as_deref(),
            loaded_settings.custom_accent_color.as_deref(),
            loaded_settings.custom_bubble_out_bg.as_deref(),
            loaded_settings.custom_bubble_in_bg.as_deref(),
        );
        initial_palette.apply_to_ctx_with_anim(
            &cc.egui_ctx,
            loaded_settings.animations_enabled,
            loaded_settings.normalized_animation_speed(),
        );

        let (tx, rx) = channel();

        // A session the server already refused is not "logged in": jump straight to the
        // login prompt instead of re-testing it (#21).
        let is_logged_in = session.login.is_some() && !session.is_expired_marked();
        let (user_name, user_uin, user_avatar) = session
            .login
            .as_ref()
            .map(|l| {
                (
                    l.display_name.clone(),
                    l.uin.clone(),
                    if l.avatar.is_empty() {
                        None
                    } else {
                        Some(l.avatar.clone())
                    },
                )
            })
            .unwrap_or_default();

        // main() already read threads_cache.json and hydrated it from SQLite; reading, merging,
        // hydrating and rewriting the cache again here doubled startup I/O.
        let mut threads = initial_threads;
        if let Some(li) = session.login.as_ref() {
            if !li.send2me_id.is_empty() {
                for t in threads.iter_mut() {
                    if !t.is_group && t.name.contains("Cloud của tôi") {
                        t.id = li.send2me_id.clone();
                    }
                }
            }
        }
        let now_ts = crate::login::unix_now() as i64;
        for t in threads.iter_mut() {
            if let Some(&until) = loaded_settings.muted_threads.get(&t.id) {
                if until == -1 || until > now_ts {
                    t.presentation.muted = true;
                    t.unread.muted = true;
                    t.unread.muted_until = (until > 0).then_some(until);
                }
            } else if t.unread.is_muted() {
                t.presentation.muted = true;
            }
        }
        crate::chat::deduplicate_threads(&mut threads, None);
        if let Ok(mut watch) = VANISH_THREADS.lock() {
            watch.clear();
            watch.extend(
                threads
                    .iter()
                    .filter(|t| t.vanish.as_ref().is_some_and(|v| v.enabled()))
                    .map(|t| t.id.clone()),
            );
        }
        let selected_thread_index = threads
            .iter()
            .enumerate()
            .max_by_key(|(_, t)| t.messages.last().map(|m| m.ts).unwrap_or(0))
            .map(|(idx, _)| idx)
            .unwrap_or(0);

        let app = Self {
            dark_mode,
            theme_dirty: false,
            cached_palette: initial_palette,
            is_logged_in,
            user_name,
            user_uin,
            user_avatar,
            pending_add_account_uin: None,
            login_tab: LoginTab::QrCode,
            phone_input: String::new(),
            password_input: String::new(),
            login_in_progress: false,
            login_status_msg: String::new(),
            qr_texture: None,
            qr_status: "Đang tạo mã QR đăng nhập…".into(),
            threads,
            reaction_inbox: crate::reactions::ReactionInbox::default(),

            selected_thread_index,            thread_filter: ThreadFilter::All,
            search_query: String::new(),
            input_message: String::new(),
            voice_recorder: None,
            replying_to: None,
            forward_msg: None,
            forward_filter: String::new(),
            reactions_dialog: None,
            reactions_tab: None,
            filter_starred_only: false,
            send_hd_original: true,
            typing_map: HashMap::new(),
            active_call: None,
            image_viewer: None,
            video_player: None,
            status_msg: "Trực tuyến · Sẵn sàng".into(),
            info_drawer_open: loaded_settings.info_drawer_open,
            profile_menu_open: false,
            wipe_confirm_open: false,
            logout_confirm_open: false,
            toast_notice: None,
            incoming_popup: None,
            phone_modal_open: false,
            phone_modal_input: String::new(),
            phone_lookup_loading: false,
            phone_lookup_result: None,
            phone_lookup_error: None,
            in_chat_search_open: false,
            in_chat_search_query: String::new(),
            in_chat_search_focus_pending: false,
            vanish_picker_open: false,
            sticker_picker_open: false,
            sticker_tab: StickerTab::Popular,
            sticker_textures: HashMap::new(),
            loading_stickers: HashSet::new(),
            failed_stickers: HashMap::new(),
            emoji_textures: load_default_emoji_textures(&cc.egui_ctx),
            app_logo_texture: load_app_logo_texture(&cc.egui_ctx),
            sticker_store_packs: Vec::new(),
            sticker_store_loading: false,
            sticker_store_attempted: false,
            sticker_store_filter: String::new(),
            sticker_pack_items: FEATURED_PACK_TABS
                .iter()
                .filter_map(|&(cid, _)| builtin_pack_stickers(cid).map(|items| (cid, items)))
                .collect(),
            loading_sticker_packs: HashSet::new(),
            failed_sticker_packs: HashSet::new(),
            pinned_sticker_packs: FEATURED_PACK_TABS
                .iter()
                .map(|&(cid, title)| (cid, title.to_string()))
                .collect(),
            sticker_search_query: String::new(),
            sticker_search_results: Vec::new(),
            sticker_search_loading: false,
            last_sticker_search_dispatched: String::new(),
            last_sticker_search_edit_at: std::time::Instant::now(),
            sticker_suggest_keywords: HashSet::new(),
            composer_sticker_cache: HashMap::new(),
            composer_sticker_loading: HashSet::new(),
            dismissed_sticker_suggest_for: String::new(),
            last_composer_text: String::new(),
            last_composer_edit_at: std::time::Instant::now(),
            composer_sticker_selected: None,
            composer_mention_selected: 0,
            dismissed_mention_suggest_for: String::new(),
            composer_focus_pending: false,
            pending_clipboard_image: None,
            unread_counts: HashMap::new(),
            reaction_unread_counts: HashMap::new(),
            link_metadata_cache: HashMap::new(),
            loading_link_previews: HashSet::new(),
            translate_pending: HashSet::new(),
            translate_generation: HashMap::new(),
            dismissed_link_preview_for: String::new(),
            friend_status_map: HashMap::new(),
            checking_friend_status: HashSet::new(),
            group_members_map: HashMap::new(),
            fetching_group_members: HashSet::new(),
            group_member_search: String::new(),
            group_note_drafts: HashMap::new(),
            group_note_loaded: HashSet::new(),
            group_note_edited_at: HashMap::new(),
            group_note_save_retry_at: HashMap::new(),
            group_note_saved_label: HashMap::new(),
            group_watch_inputs: HashMap::new(),
            group_watch_cache: HashMap::new(),
            group_stats_cache: HashMap::new(),
            group_note_loading: HashSet::new(),
            group_note_load_error: HashMap::new(),
            group_stats_loading: HashSet::new(),
            group_summary_pending: None,
            group_jump_pending: None,
            script_tab: 0,
            script_list_cache: HashMap::new(),
            script_logs_cache: HashMap::new(),
            script_loading_threads: HashSet::new(),
            script_editing_id: None,
            script_name_draft: String::new(),
            script_trigger_draft: "manual".to_string(),
            script_arg_draft: String::new(),
            script_code_draft: String::new(),
            script_output: String::new(),
            pending_delete_thread: None,
            pending_leave_group: None,
            doc_previews: HashMap::new(),
            doc_preview_loading: HashSet::new(),
            doc_preview_errors: HashMap::new(),
            doc_viewer: None,
            user_online_map: HashMap::new(),
            allow_quit: false,
            settings_modal_open: false,
            settings_tab: SettingsTab::General,
            notification_authorization: Default::default(),
            settings: loaded_settings,
            settings_blur_tex: None,
            settings_blur_req_at: None,
            update_checking: false,
            update_downloading: false,
            update_progress: None,
            update_status: String::new(),
            update_info: None,
            update_modal_open: false,
            update_banner_dismissed: false,
            last_notified_update_version: None,
            dev_mode,
            dev_panel_open: false,
            dev_tab: DevTab::Logs,
            selected_log: None,
            scanner_findings: Vec::new(),
            feed_modal_open: false,
            feed_modal_tab: FeedModalTab::Profile,
            feed_target_uid: String::new(),
            feed_loading_profile: false,
            feed_profile: None,
            feed_profile_error: None,
            feed_loading_avatars: false,
            feed_avatars: Vec::new(),
            feed_avatars_error: None,
            feed_loading_timeline: false,
            feed_items: Vec::new(),
            feed_friend_profiles: Vec::new(),
            feed_search_query: String::new(),
            feed_pkt_owner_id: String::new(),
            feed_pkt_viewer_id: String::new(),
            feed_pkt_tab: 0,
            feed_pkt_page: 1,
            feed_pkt_count: 20,
            contacts_view_open: false,
            contacts_sub_tab: ContactsSubTab::Friends,
            contacts_search_query: String::new(),
            schedule_modal_open: false,
            schedule_custom_input: String::new(),
            schedule_editing_id: None,
            scheduled_items: crate::scheduler::list_pending_scheduled("zalo_history.db", None)
                .unwrap_or_default(),
            phone_sync_progress: 0.0,
            phone_sync_status: String::new(),
            saved_modal_open: false,
            saved_items: Vec::new(),
            saved_note_drafts: std::collections::HashMap::new(),
            saved_due_drafts: std::collections::HashMap::new(),
            draft_map: HashMap::new(),
            draft_reply_map: HashMap::new(),
            draft_threads: HashSet::new(),
            reminder_modal_open: false,
            reminder_items: Vec::new(),
            reminder_due_pending: Vec::new(),
            remind_pick_msg: None,
            remind_custom_open: false,
            remind_custom_input: String::new(),
            editing_message: None,
            edit_history_cache: HashMap::new(),
            edit_history_open: None,
            folder_modal_open: false,
            last_typing_sent_at: None,
            last_synced_icon_unread: None,
            event_rx: rx,
            event_tx: tx.clone(),
            chat_render_window: (String::new(), CHAT_RENDER_STEP),
            quote_jump: None,
            ime_active: false,
            ime_just_committed: false,
            history_reached_start: HashSet::new(),
            force_scroll_to_bottom: false,
            #[cfg(target_os = "windows")]
            win_hwnd: {
                let hwnd = win32_frameless::find_window_hwnd();
                let _ = win32_frameless::install_frameless_subclass(hwnd);
                hwnd
            },
            #[cfg(target_os = "windows")]
            win_maximized: false,
            #[cfg(target_os = "windows")]
            win_restore_rect: None,
            #[cfg(target_os = "windows")]
            win_resize_drag: None,
        };
        let mut app = app;
        let _ = app.reload_custom_sticker_packs();

        // Clean up any leftover `.old` executable from a previous in-place self-update
        if let Ok(exe_path) = std::env::current_exe() {
            let old_path = exe_path.with_extension("exe.old");
            let _ = std::fs::remove_file(&old_path);
            let old_unix = exe_path.with_extension("old");
            let _ = std::fs::remove_file(&old_unix);
        }

        app.publish_known_threads();
        spawn_session_worker(
            session,
            GuiTx {
                tx,
                ctx: cc.egui_ctx.clone(),
            },
            cc.egui_ctx.clone(),
        );
        ensure_cjk_font(&cc.egui_ctx, thread_texts(&app.threads));

        if let Some(wtx) = WORKER_TX.get() {
            if is_logged_in {
                let _ = wtx.send(WorkerCmd::RefreshThreads);
            } else {
                let _ = wtx.send(WorkerCmd::RequestQr);
            }
            let _ = wtx.send(WorkerCmd::CheckForUpdates { silent: true });
        }
        app
    }
}

/// Maximum retries after an initial sticker/image download failure (1 initial + 2 retries = 3 attempts).
const MAX_STICKER_FETCH_ATTEMPTS: u8 = 3;
/// Cooldown window before retrying a failed sticker/image download.
const STICKER_RETRY_COOLDOWN: Duration = Duration::from_secs(30);

/// Returns `true` when `key` has not exceeded retry limits and its cooldown elapsed.
pub(crate) fn can_attempt_sticker_fetch(
    failures: &HashMap<String, (u8, std::time::Instant)>,
    key: &str,
    now: std::time::Instant,
) -> bool {
    match failures.get(key) {
        None => true,
        Some(&(attempts, last_fail)) => {
            attempts < MAX_STICKER_FETCH_ATTEMPTS
                && now.saturating_duration_since(last_fail) >= STICKER_RETRY_COOLDOWN
        }
    }
}

/// Records a sticker/image fetch failure with timestamp and capped attempt count.
pub(crate) fn record_sticker_fetch_failure(
    failures: &mut HashMap<String, (u8, std::time::Instant)>,
    key: &str,
    now: std::time::Instant,
) {
    let entry = failures.entry(key.to_string()).or_insert((0, now));
    entry.0 = entry.0.saturating_add(1);
    entry.1 = now;
}

/// Queues a worker download for a sticker/avatar/thumbnail without borrowing the full `ZaloGuiApp`.
pub(crate) fn queue_sticker_fetch(
    sticker_textures: &HashMap<String, TextureHandle>,
    loading_stickers: &mut HashSet<String>,
    failed_stickers: &HashMap<String, (u8, std::time::Instant)>,
    key: String,
    url: String,
) {
    if url.is_empty()
        || sticker_textures.contains_key(&key)
        || loading_stickers.contains(&key)
        || !can_attempt_sticker_fetch(failed_stickers, &key, std::time::Instant::now())
    {
        return;
    }
    loading_stickers.insert(key.clone());
    if let Some(tx) = WORKER_TX.get() {
        let _ = tx.send(WorkerCmd::FetchStickerImage { key, url });
    }
}

impl ZaloGuiApp {
    /// Queues a worker download for a sticker/avatar/thumbnail once retry backoff allows it.
    pub(super) fn request_sticker_image(&mut self, key: String, url: String) {
        queue_sticker_fetch(
            &self.sticker_textures,
            &mut self.loading_stickers,
            &self.failed_stickers,
            key,
            url,
        );
    }

    pub(super) fn refresh_cached_palette(&mut self) {
        let mut pal = ThemePalette::load_with_custom_override(self.dark_mode);
        pal.apply_custom_colors(
            self.settings.custom_primary_color.as_deref(),
            self.settings.custom_accent_color.as_deref(),
            self.settings.custom_bubble_out_bg.as_deref(),
            self.settings.custom_bubble_in_bg.as_deref(),
        );
        self.cached_palette = pal;
    }

    fn palette(&self) -> ThemePalette {
        self.cached_palette
    }

    pub(super) fn anim_value(&self, ctx: &egui::Context, id: egui::Id, target: f32, base_secs: f32) -> f32 {
        crate::gui::theme::animate_value_scaled(
            ctx,
            id,
            target,
            base_secs,
            self.settings.animations_enabled,
            self.settings.normalized_animation_speed(),
        )
    }

    pub(super) fn anim_bool(&self, ctx: &egui::Context, id: egui::Id, value: bool, base_secs: f32) -> f32 {
        crate::gui::theme::animate_bool_scaled(
            ctx,
            id,
            value,
            base_secs,
            self.settings.animations_enabled,
            self.settings.normalized_animation_speed(),
        )
    }

    pub(super) fn reload_custom_sticker_packs(&mut self) -> usize {
        let packs = load_custom_sticker_packs();
        let count = packs.len();
        for p in packs {
            if !self
                .pinned_sticker_packs
                .iter()
                .any(|(cid, _)| *cid == p.cate_id)
            {
                self.pinned_sticker_packs.push((p.cate_id, p.name.clone()));
            }
            if let Some(items) = builtin_pack_stickers(p.cate_id) {
                self.sticker_pack_items.insert(p.cate_id, items);
            }
        }
        count
    }

    pub(super) fn refresh_scheduled_items(&mut self) {
        self.scheduled_items =
            crate::scheduler::list_pending_scheduled("zalo_history.db", None).unwrap_or_default();
    }
    /// Tải lại hộp Đã lưu từ DB acc hiện tại (WaveA2).
    pub(super) fn refresh_saved_items(&mut self) {
        let db = crate::store::db_path_for_account(&self.user_uin);
        self.saved_items = crate::store::MessageStore::open(&db)
            .map(|st| st.list_saved_items())
            .unwrap_or_default();
    }

    /// Bật/tắt lưu tin + đồng bộ star 2 chiều (WaveA2).
    pub(super) fn toggle_saved_for(&mut self, thread_id: &str, msg_id: &str, starred: bool) {
        let db = crate::store::db_path_for_account(&self.user_uin);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        if let Ok(mut st) = crate::store::MessageStore::open(&db) {
            if starred {
                let _ = st.save_message_item(msg_id, thread_id, now);
            } else {
                let _ = st.unsave_message_item(msg_id);
            }
        }
        self.refresh_saved_items();
    }
    /// Toast các tin đã lưu tới hạn (WaveA2). Gọi mỗi lần mở hộp Đã lưu / tick 2s.
    /// `due_at` nối scheduler: dùng format_schedule_time + toast, không gửi tin.
    pub(super) fn toast_due_saved(&mut self) {
        let now = chrono::Local::now().timestamp();
        let db = crate::store::db_path_for_account(&self.user_uin);
        let due = crate::store::MessageStore::open(&db)
            .map(|st| st.due_saved_items(now))
            .unwrap_or_default();
        for s in due {
            self.show_toast(format!("⏰ Đã lưu tới hạn ({}): {}", crate::scheduler::format_schedule_time(s.due_at), s.text.chars().take(60).collect::<String>()));
        }
    }
    /// Lưu nháp composer hiện tại của thread cũ vào RAM + SQLite (WaveA1).
    /// Gọi trước mọi đổi thread. Text rỗng + không reply → xoá row.
    pub(super) fn save_current_draft(&mut self) {
        let old_tid = match self.threads.get(self.selected_thread_index) {
            Some(t) => t.id.clone(),
            None => return,
        };
        let text = self.input_message.clone();
        let reply_ref = self
            .replying_to
            .as_ref()
            .map(|m| m.id.clone())
            .unwrap_or_default();
        if text.trim().is_empty() && reply_ref.trim().is_empty() {
            self.draft_map.remove(&old_tid);
            self.draft_reply_map.remove(&old_tid);
            self.draft_threads.remove(&old_tid);
        } else {
            if !text.trim().is_empty() {
                self.draft_map.insert(old_tid.clone(), text.clone());
            } else {
                self.draft_map.remove(&old_tid);
            }
            if !reply_ref.trim().is_empty() {
                self.draft_reply_map.insert(old_tid.clone(), reply_ref.clone());
            } else {
                self.draft_reply_map.remove(&old_tid);
            }
            self.draft_threads.insert(old_tid.clone());
        }
        let db = crate::store::db_path_for_account(&self.user_uin);
        let now = crate::login::unix_now() as i64;
        std::thread::spawn(move || {
            if let Ok(mut st) = crate::store::MessageStore::open(&db) {
                let _ = st.set_draft(&old_tid, &text, &reply_ref, now);
            }
        });
    }

    /// Nạp nháp của thread mới vào composer + reply banner (WaveA1).
    pub(super) fn load_draft_for(&mut self, thread_id: &str) {
        if let Some(text) = self.draft_map.get(thread_id).cloned() {
            self.input_message = text;
        } else {
            self.input_message.clear();
        }
        if let Some(ref_id) = self.draft_reply_map.get(thread_id).cloned() {
            let found = self
                .threads
                .iter()
                .find(|t| t.id == thread_id)
                .and_then(|t| t.messages.iter().find(|m| m.id == ref_id).cloned());
            self.replying_to = found;
            if self.replying_to.is_none() {
                self.draft_reply_map.remove(thread_id);
            }
        } else {
            self.replying_to = None;
        }
        self.dismissed_sticker_suggest_for.clear();
        self.composer_sticker_selected = None;
        self.composer_focus_pending = false;
    }

    /// Đổi thread có giữ nháp: lưu cũ → đổi index → nạp mới (WaveA1).
    pub(super) fn switch_thread_with_draft(&mut self, new_index: usize) {
        if new_index >= self.threads.len() {
            return;
        }
        let old_tid = self
            .threads
            .get(self.selected_thread_index)
            .map(|t| t.id.clone())
            .unwrap_or_default();
        let new_tid = self.threads[new_index].id.clone();
        if old_tid == new_tid {
            self.selected_thread_index = new_index;
            return;
        }
        if let Some(em) = self.editing_message.take() {
            self.input_message = em.draft;
        }
        self.save_current_draft();
        self.selected_thread_index = new_index;
        self.load_draft_for(&new_tid);
        self.filter_starred_only = false;
        self.force_scroll_to_bottom = true;
        self.chat_render_window = (new_tid, CHAT_RENDER_STEP);
    }

    /// Tải lại badge nháp + list reminder từ DB acc hiện tại (WaveA1).
    pub(super) fn refresh_draft_reminder_cache(&mut self) {
        let db = crate::store::db_path_for_account(&self.user_uin);
        if let Ok(store) = crate::store::MessageStore::open(&db) {
            self.draft_threads = store.list_draft_threads().into_iter().collect();
            self.reminder_items = store.list_reminders();
        } else {
            self.draft_threads.clear();
            self.reminder_items.clear();
        }
        // Nạp nháp RAM từ DB cho các thread chưa có (mở app lại vẫn giữ nháp).
        if let Ok(store) = crate::store::MessageStore::open(&db) {
            for tid in self.draft_threads.clone() {
                if !self.draft_map.contains_key(&tid) {
                    if let Some(d) = store.get_draft(&tid) {
                        if !d.text.is_empty() {
                            self.draft_map.insert(tid.clone(), d.text);
                        }
                        if !d.reply_ref.is_empty() {
                            self.draft_reply_map.insert(tid, d.reply_ref);
                        }
                    }
                }
            }
        }
    }

    /// Nhảy tới tin của reminder (đổi thread giữ nháp + QuoteJump highlight) (WaveA1).
    pub(super) fn jump_to_reminder(&mut self, thread_id: &str, msg_id: &str) {
        let idx = match self.threads.iter().position(|t| t.id == thread_id) {
            Some(i) => i,
            None => {
                self.show_toast("Hội thoại của tin nhắc không còn trong danh sách");
                return;
            }
        };
        let reference = msg_id.to_string();
        let has_msg = self.threads[idx]
            .messages
            .iter()
            .any(|m| crate::message_navigation::message_matches_reference(m, &reference));
        self.switch_thread_with_draft(idx);
        self.reminder_due_pending
            .retain(|r| !(r.thread_id == thread_id && r.msg_id == msg_id));
        if has_msg {
            self.filter_starred_only = false;
            self.in_chat_search_open = false;
            self.in_chat_search_query.clear();
            self.quote_jump = Some(QuoteJump {
                thread_id: thread_id.to_string(),
                reference,
                highlight_until: std::time::Instant::now() + std::time::Duration::from_secs(5),
                scroll_pending: true,
            });
        } else {
            self.show_toast("Chưa tải tin nhắn gốc — hãy tải thêm lịch sử rồi thử lại.");
        }
    }

    /// Hand the worker's poll timer the thread ids/names it maps incoming messages onto.
    fn publish_known_threads(&self) {
        if let Ok(mut known) = KNOWN_THREADS.lock() {
            *known = self
                .threads
                .iter()
                .map(|t| (t.id.clone(), t.name.clone(), t.is_group))
                .collect();
        }
    }
    /// Rebuild `VANISH_THREADS` from the current RAM threads (gui.rs:875).
    /// Called after account switch / hydrate / ThreadsUpdated replace, and to
    /// clear on logout / wipe. Keep in `gui.rs` (not worker) so GUI and sweep
    /// always agree on the watched set.
    fn refresh_vanish_watch(&self) {
        if let Ok(mut watch) = VANISH_THREADS.lock() {
            watch.clear();
            watch.extend(
                self.threads
                    .iter()
                    .filter(|t| t.vanish.as_ref().is_some_and(|v| v.enabled()))
                    .map(|t| t.id.clone()),
            );
        }
    }

    /// Re-read the pinned board of the open thread, throttled.
    ///
    /// Parked for now: `friendboard/list` rejects the request shapes we can build (error 114), so
    /// polling only produced a toast on every thread switch. The read-back will come from the
    /// socket control events (`pin_message` / `unpin_topic`) instead.
    fn maybe_refresh_board_pins(&mut self) {}

    fn persist_threads_async(&self) {
        self.publish_known_threads();
        let snapshot = self.threads.clone();
        if let Some(wtx) = WORKER_TX.get() {
            let _ = wtx.send(WorkerCmd::PersistThreads(snapshot));
        }
    }
    /// Chạy script on_message/on_keyword cho các tin mới (worker thread, không block UI).
    /// Chỉ script đúng thread + đúng trigger mới chạy; outbox drain qua WorkerCmd::SendMessage.
    fn run_message_scripts(
        db: &str,
        _uin: &str,
        thread_id: &str,
        snapshot: &[Thread],
        fresh: &[ChatMessage],
        tx: &std::sync::mpsc::Sender<GuiEvent>,
    ) {
        let store = match crate::store::MessageStore::open(db) {
            Ok(s) => s,
            Err(_) => return,
        };
        let scripts = store.list_thread_scripts(thread_id);
        if scripts.is_empty() {
            return;
        }
        for s in scripts.iter().filter(|s| s.enabled) {
            let kind = crate::thread_script::parse_trigger(&s.trigger, &s.trigger_arg);
            for m in fresh {
                if !crate::thread_script::trigger_fires_on_message(&kind, &m.text) {
                    continue;
                }
                let ev = crate::thread_script::ScriptEvent::message(&m.text, &m.sender);
                match crate::thread_script::run_script(db, snapshot, thread_id, s, &ev) {
                    Ok(out) => {
                        for o in out.outbox {
                            if let Some(wtx) = WORKER_TX.get() {
                                // Tái dùng SendMessage có sẵn; is_group suy từ snapshot.
                                let is_group = snapshot
                                    .iter()
                                    .find(|t| t.id == o.thread_id)
                                    .map(|t| t.is_group)
                                    .unwrap_or(false);
                                let _ = wtx.send(WorkerCmd::SendMessage {
                                    to_id: o.thread_id.clone(),
                                    text: o.text.clone(),
                                    is_group,
                                });
                            }
                        }
                        for t in out.toasts {
                            let _ = tx.send(GuiEvent::ScriptToast {
                                thread_id: thread_id.to_string(),
                                message: t,
                            });
                        }
                        if !out.output.is_empty() {
                            let _ = tx.send(GuiEvent::ScriptToast {
                                thread_id: thread_id.to_string(),
                                message: format!("⚙️ {}: {}", s.name, out.output),
                            });
                        }
                        let _ = tx.send(GuiEvent::ScriptLogsUpdated {
                            thread_id: thread_id.to_string(),
                        });
                    }
                    Err(e) => {
                        crate::thread_script::persist_error_log(db, thread_id, &s.name, &e.to_string());
                        let _ = tx.send(GuiEvent::ScriptLogsUpdated {
                            thread_id: thread_id.to_string(),
                        });
                    }
                }
            }
        }
    }


    /// Lưu ngay mọi ghi chú nhóm đang treo debounce (không chờ đủ ~1s).
    /// Gọi khi đổi nhóm / nhận merge / đóng drawer, để không mất chữ đã gõ.
    /// Chạy đồng bộ trên UI thread: 1 UPDATE SQLite nhỏ, rẻ như autosave.
    fn flush_group_note_pending(&mut self) {
        if self.group_note_edited_at.is_empty() {
            return;
        }
        let ids: Vec<String> = self.group_note_edited_at.keys().cloned().collect();
        for tid in ids {
            let draft = self.group_note_drafts.get(&tid).cloned().unwrap_or_default();
            let db = crate::store::db_path_for_account(&self.user_uin);
            if let Ok(mut st) = crate::store::MessageStore::open(&db) {
                if st.set_group_note(&tid, &draft, chrono::Local::now().timestamp()).is_ok() {
                    let saved_at =
                        crate::clock::message_time(chrono::Local::now().timestamp_millis());
                    self.group_note_saved_label.insert(tid.clone(), format!("Đã lưu {saved_at}"));
                    self.group_note_edited_at.remove(&tid);
                }
            }
        }
    }

    fn check_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                GuiEvent::NotificationAuthorization { request_id, result } => {
                    self.notification_authorization.finish(request_id, result);
                }
                GuiEvent::QrReady { qr_image, .. } => {
                    self.qr_texture =
                        Some(ctx.load_texture("qr_code_tex", qr_image, TextureOptions::NEAREST));
                    self.qr_status = "Mở ứng dụng Zalo trên điện thoại và quét mã QR".into();
                    self.login_in_progress = false;
                }
                GuiEvent::QrScanned { name } => {
                    self.qr_status =
                        format!("Đã quét bởi {name} — Vui lòng bấm 'Đăng nhập' trên điện thoại…");
                }
                GuiEvent::LoginSuccess { name, uin, avatar } => {
                    // Đổi acc: flush note pending của acc cũ TRƯỚC khi đổi identity,
                    // rồi clear cache workspace để acc mới không thấy/đè state cũ.
                    if self.user_uin != uin {
                        self.flush_group_note_pending();
                        self.group_note_drafts.clear();
                        self.group_note_loaded.clear();
                        self.group_note_edited_at.clear();
                        self.group_note_save_retry_at.clear();
                        self.group_note_saved_label.clear();
                        self.group_watch_inputs.clear();
                        self.group_watch_cache.clear();
                        self.group_stats_cache.clear();
                        self.script_list_cache.clear();
                        self.script_editing_id = None;
                        self.script_output.clear();
                        self.group_note_loading.clear();
                        self.group_note_load_error.clear();
                        self.group_stats_loading.clear();
                        self.group_summary_pending = None;
                        self.group_jump_pending = None;
                    }
                    // Refresh (đã login) thì merge, KHÔNG clear — clear + nạp lại sẽ
                    // khiến nhánh brand-new đếm toàn bộ history thành unread (#119).
                    let is_refresh = self.is_logged_in;
                    self.is_logged_in = true;
                    self.pending_add_account_uin = None;
                    self.user_name = name;
                    self.user_uin = uin;
                    if avatar.is_some() {
                        self.user_avatar = avatar;
                    }
                    if !is_refresh {
                        self.threads.clear();
                        self.unread_counts.clear();
                        self.reaction_unread_counts.clear();
                        self.history_reached_start.clear();
                        self.selected_thread_index = 0;
                    }
                    self.login_in_progress = false;
                    self.composer_sticker_cache.retain(|_, v| !v.is_empty());
                    self.status_msg = "Trực tuyến · Đồng bộ thời gian thực".into();
                }
                GuiEvent::LoginFailed(err) => {
                    self.login_in_progress = false;
                    self.login_status_msg = format!("Lỗi đăng nhập: {err}");
                }
                GuiEvent::ThreadsUpdated(updates) => {
                    ensure_cjk_font(ctx, thread_texts(&updates));
                    if self.is_logged_in && !updates.is_empty() {
                        // Group Workspace: flush ghi chú đang treo (debounce) trước khi merge —
                        // đổi nhóm nhanh (drawer chưa mở/đóng) không mất chữ đã gõ.
                        self.flush_group_note_pending();
                        let prev_selected = self.threads.get(self.selected_thread_index).cloned();
                        let live_ids: HashSet<String> = self
                            .threads
                            .iter()
                            .chain(updates.iter())
                            .filter(|u| !u.id.is_empty() && u.id != "0")
                            .map(|u| u.id.clone())
                            .collect();
                        let update_ids: HashSet<String> = updates
                            .iter()
                            .filter(|u| !u.id.is_empty() && u.id != "0")
                            .map(|u| u.id.clone())
                            .collect();

                        let active_tid = prev_selected
                            .as_ref()
                            .map(|t| t.id.clone())
                            .unwrap_or_default();
                        let is_focused = ctx.input(|i| i.focused);
                        let mut new_incoming_alert = false;
                        let mut chat_banners: Vec<(String, String)> = Vec::new();
                        // Group Workspace: toast khớp keyword gom ở đây để bắn sau merge
                        // (tránh borrow `existing` + `self` cùng lúc trong vòng lặp).
                        let mut watch_toasts: Vec<String> = Vec::new();
                        // Script on_message: (thread_id, tin mới) chờ chạy sau merge (tránh borrow).
                        let mut script_msg_pending: Vec<(String, Vec<ChatMessage>)> = Vec::new();
                        let now_ts = crate::login::unix_now() as i64;
                        for mut nt in updates {
                            let settings_muted_until = self
                                .settings
                                .muted_threads
                                .get(&nt.id)
                                .copied()
                                .filter(|&u| u == -1 || u > now_ts);
                            if let Some(until) = settings_muted_until {
                                nt.presentation.muted = true;
                                nt.unread.muted = true;
                                nt.unread.muted_until = (until > 0).then_some(until);
                            } else if nt.unread.is_muted() {
                                nt.presentation.muted = true;
                            }
                            if let Some(existing) = self.threads.iter_mut().find(|t| t.id == nt.id)
                            {
                                let is_thread_muted = settings_muted_until.is_some()
                                    || existing.presentation.muted
                                    || existing.unread.is_muted()
                                    || nt.presentation.muted
                                    || nt.unread.is_muted();
                                let old_len = existing.messages.len();
                                let old_ids: std::collections::HashSet<String> = existing
                                    .messages
                                    .iter()
                                    .filter(|m| !m.id.is_empty())
                                    .map(|m| m.id.clone())
                                    .collect();
                                crate::chat::merge_thread_into(existing, nt, true);
                                let added = existing.messages.len().saturating_sub(old_len);
                                if added > 0 {
                                    let seen = existing.unread.last_seen_id.as_deref().unwrap_or("");
                                    let inc_cnt = existing
                                        .messages
                                        .iter()
                                        .rev()
                                        .take(added)
                                        .filter(|m| {
                                            if m.is_outgoing {
                                                return false;
                                            }
                                            if seen.is_empty() {
                                                return true;
                                            }
                                            matches!(
                                                crate::unread::compare_ids(&m.id, seen),
                                                Some(std::cmp::Ordering::Greater) | None
                                            )
                                        })
                                        .count();
                                    if inc_cnt > 0 {
                                        if !is_thread_muted {
                                            new_incoming_alert = true;
                                        }
                                        if existing.id != active_tid || !is_focused {
                                            *self
                                                .unread_counts
                                                .entry(existing.id.clone())
                                                .or_insert(0) += inc_cnt;
                                            if !is_thread_muted {
                                                if let Some(last_inc) = existing
                                                    .messages
                                                    .iter()
                                                    .rev()
                                                    .take(added)
                                                    .find(|m| !m.is_outgoing)
                                                {
                                                    let sender_label = if existing.is_group
                                                        && !last_inc.sender.is_empty()
                                                    {
                                                        format!(
                                                            "{} · {}",
                                                            last_inc.sender, existing.name
                                                        )
                                                    } else {
                                                        existing.name.clone()
                                                    };
                                                    let preview_raw = last_inc.text.trim();
                                                    let preview_short: String =
                                                        if preview_raw.chars().count() > 96 {
                                                            let s: String = preview_raw
                                                                .chars()
                                                                .take(96)
                                                                .collect();
                                                            format!("{s}…")
                                                        } else if preview_raw.is_empty() {
                                                            "[ Đính kèm / Nhãn dán ]".to_string()
                                                        } else {
                                                            preview_raw.to_string()
                                                        };
                                                    let avt_url = existing
                                                        .avatar
                                                        .clone()
                                                        .filter(|u| !u.trim().is_empty());
                                                    if let Some(ref url) = avt_url {
                                                        let tex_key = format!(
                                                            "thumb_{}",
                                                            crate::crypto::md5_hex(url)
                                                        );
                                                        queue_sticker_fetch(
                                                            &self.sticker_textures,
                                                            &mut self.loading_stickers,
                                                            &self.failed_stickers,
                                                            tex_key,
                                                            url.clone(),
                                                        );
                                                    }
                                                    let unread_for_thread = self
                                                        .unread_counts
                                                        .get(&existing.id)
                                                        .copied()
                                                        .unwrap_or(1);
                                                    self.incoming_popup =
                                                        Some(IncomingPopupState {
                                                            thread_id: existing.id.clone(),
                                                            sender_name: sender_label.clone(),
                                                            preview_text: preview_short.clone(),
                                                            avatar_url: avt_url,
                                                            is_group: existing.is_group,
                                                            unread_total: unread_for_thread,
                                                            created_at: std::time::Instant::now(),
                                                            reply_draft: String::new(),
                                                        });
                                                    chat_banners.push((sender_label, preview_short));
                                                }
                                            }
                                        }
                                    }
                                    // Group Workspace: tin mới khớp từ khóa → toast ưu tiên.
                                    // Chỉ tin live (id chưa từng thấy) mới match — history
                                    // load (LoadGroupHistory merge) không bắn toast trùng.
                                    // Tôn trọng mute thread + toggle notifications; không cộng
                                    // unread riêng cho watch (tin đã có unread thread).
                                    if existing.is_group {
                                        let fresh: Vec<&ChatMessage> = existing
                                            .messages
                                            .iter()
                                            .filter(|m| {
                                                !m.is_outgoing
                                                    && (m.id.is_empty() || !old_ids.contains(&m.id))
                                            })
                                            .collect();
                                        if !fresh.is_empty() {
                                            let hits =
                                                crate::group_watch::match_new_messages(
                                                    &self.user_uin,
                                                    &existing.id,
                                                    &fresh,
                                                );
                                            if !hits.is_empty()
                                                && !is_thread_muted
                                                && self.settings.enable_notifications
                                            {
                                                for h in hits {
                                                    watch_toasts.push(format!(
                                                        "🔔 [{}] {}: {}",
                                                        h.keyword, h.sender, h.preview
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                    // Script per-thread trigger on_message/on_keyword: gom vào hàng đợi
                                    // (tránh borrow self.threads trong iter_mut), chạy sau merge.
                                    {
                                        let fresh_msgs: Vec<ChatMessage> = existing
                                            .messages
                                            .iter()
                                            .filter(|m| {
                                                !m.is_outgoing
                                                    && (m.id.is_empty() || !old_ids.contains(&m.id))
                                            })
                                            .cloned()
                                            .collect();
                                        if !fresh_msgs.is_empty() {
                                            script_msg_pending.push((
                                                existing.id.clone(),
                                                fresh_msgs,
                                            ));
                                        }
                                    }
                                    if existing.id == self.chat_render_window.0 {
                                        self.chat_render_window.1 += added;
                                    }
                                }
                            } else if let Some(existing) = self.threads.iter_mut().find(|t| {
                                !update_ids.contains(&t.id)
                                    && crate::chat::threads_represent_same_contact(t, &nt)
                            }) {
                                crate::chat::merge_thread_into(existing, nt, true);
                            } else if !nt.id.is_empty() && nt.id != "0" {
                                // Brand-new thread chưa có read-point (#1445): đây là lần
                                // đầu thấy thread (history load hoặc poll đầu), KHÔNG
                                // đếm toàn bộ history thành unread và không banner tin cũ.
                                // Lựa chọn: seed read-point = tin mới nhất rồi đếm 0 —
                                // lần đầu mở app không báo ồ ạt, tin mới sau đó (WS
                                // event / poll batch tiếp theo) mới đếm qua nhánh
                                // merge ở trên. Đơn giản và đúng vì mọi tin trong
                                // batch đầu đều là history, không phải tin mới.
                                let is_thread_muted = settings_muted_until.is_some()
                                    || nt.presentation.muted
                                    || nt.unread.is_muted();
                                let seen = nt.unread.last_seen_id.as_deref().unwrap_or("");
                                if seen.is_empty() {
                                    if let Some(newest) = nt.messages.last() {
                                        nt.unread.last_seen_id = Some(newest.id.clone());
                                    }
                                    self.threads.push(nt);
                                    continue;
                                }
                                let inc_cnt = nt
                                    .messages
                                    .iter()
                                    .filter(|m| {
                                        if m.is_outgoing {
                                            return false;
                                        }
                                        // Tin mới hơn read-point, hoặc id không so được
                                        // (non-numeric) thì đếm cho chắc.
                                        matches!(
                                            crate::unread::compare_ids(&m.id, seen),
                                            Some(std::cmp::Ordering::Greater) | None
                                        )
                                    })
                                    .count();
                                if inc_cnt > 0 && (nt.id != active_tid || !is_focused) {
                                    *self.unread_counts.entry(nt.id.clone()).or_insert(0) +=
                                        inc_cnt;
                                }
                                if !is_thread_muted
                                    && inc_cnt > 0
                                    && (nt.id != active_tid || !is_focused)
                                {
                                    new_incoming_alert = true;
                                    if let Some(latest) = nt
                                        .messages
                                        .iter()
                                        .rev()
                                        .find(|m| !m.is_outgoing)
                                    {
                                        let sender = if nt.is_group && !latest.sender.is_empty() {
                                            format!("{} · {}", latest.sender, nt.name)
                                        } else {
                                            nt.name.clone()
                                        };
                                        let preview_raw = latest.text.trim();
                                        let preview_short: String =
                                            if preview_raw.chars().count() > 96 {
                                                let s: String = preview_raw
                                                    .chars()
                                                    .take(96)
                                                    .collect();
                                                format!("{s}…")
                                            } else if preview_raw.is_empty() {
                                                "[ Đính kèm / Nhãn dán ]".to_string()
                                            } else {
                                                preview_raw.to_string()
                                            };
                                        chat_banners.push((sender, preview_short));
                                    }
                                }
                                self.threads.push(nt);
                            }
                        }
                        // Group Workspace: bắn toast khớp keyword sau merge (ưu tiên, sau cùng thắng).
                        for toast in watch_toasts {
                            self.show_toast(toast);
                        }
                        if new_incoming_alert && self.settings.enable_notifications {
                            if self.settings.play_notification_sound {
                                crate::media::play_notification_chime();
                            }
                            crate::os_notify::notify_chat_batch(&chat_banners);
                            let total_unread: usize = self.unread_counts.values().sum();
                            let title = if total_unread > 0 {
                                format!("({total_unread}) ctdozlite")
                            } else {
                                "ctdozlite".to_string()
                            };
                            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
                            if !is_focused {
                                ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                                    egui::UserAttentionType::Informational,
                                ));
                            }
                        }
                        crate::chat::deduplicate_threads(&mut self.threads, Some(&live_ids));
                        // Restore selected thread by exact `id` first so contacts sharing the
                        // same Zalo display name never jump to each other's chat window (#63).
                        if let Some(prev) = prev_selected {
                            if let Some(idx) = self.threads.iter().position(|t| t.id == prev.id) {
                                self.selected_thread_index = idx;
                            } else if let Some(idx) = self
                                .threads
                                .iter()
                                .position(|t| crate::chat::threads_represent_same_contact(t, &prev))
                            {
                                self.selected_thread_index = idx;
                            }
                        }
                        if self.selected_thread_index >= self.threads.len() {
                            self.selected_thread_index = self.threads.len().saturating_sub(1);
                        }
                        // A merge may bring a stale history total; the event ledger is authoritative.
                        if !self.reaction_inbox.is_empty() {
                            for t in self.threads.iter_mut() {
                                self.reaction_inbox.apply_to(&mut t.messages);
                            }
                        }
                        self.refresh_vanish_watch();
                        self.persist_threads_async();
                        // Script on_message/on_keyword: gate rẻ theo DB rồi chạy trên RAM snapshot.
                        if !script_msg_pending.is_empty() {
                            let db = crate::store::db_path_for_account(&self.user_uin);
                            let gated: Vec<(String, Vec<ChatMessage>)> =
                                crate::store::MessageStore::open(&db)
                                    .map(|s| {
                                        script_msg_pending
                                            .into_iter()
                                            .filter(|(tid, _)| s.thread_has_message_scripts(tid))
                                            .collect()
                                    })
                                    .unwrap_or_default();
                            if !gated.is_empty() {
                                let snapshot = self.threads.clone();
                                let tx = self.event_tx.clone();
                                let uin = self.user_uin.clone();
                                std::thread::spawn(move || {
                                    for (tid, fresh) in &gated {
                                        Self::run_message_scripts(
                                            &db, &uin, tid, &snapshot, fresh, &tx,
                                        );
                                    }
                                });
                            }
                        }
                        // A pin/unpin from another device arrives only as an event message, so the
                        // board has to be re-read before the banner/chips can match the server.
                        self.maybe_refresh_board_pins();
                    }
                    self.status_msg = format!("Trực tuyến · Cập nhật {}", crate::ui::clock());
                }
                GuiEvent::SessionExpired(reason) => {
                    self.is_logged_in = false;
                    self.qr_texture = None;
                    self.qr_status =
                        format!("{reason} — Quét mã QR bên dưới để khôi phục phiên trực tuyến.");
                    self.status_msg = format!("{reason} (Lịch sử tin nhắn vẫn được bảo toàn)");
                }
                GuiEvent::SessionUnreachable(reason) => {
                    // Only the network failed: stay logged in, keep the thread list, and
                    // let the next poll retry (#21).
                    self.status_msg = format!("⚠️ {reason} — đang thử lại…");
                }
                GuiEvent::LocalDataWiped => {
                    self.threads.clear();
                    self.selected_thread_index = 0;
                    // Sweep không còn gì để quét — clear để đổi acc sau không quét nhầm (#875).
                    if let Ok(mut watch) = VANISH_THREADS.lock() {
                        watch.clear();
                    }
                    // Group Workspace cache theo DB cũ → xoá cùng.
                    self.group_note_drafts.clear();
                    self.group_note_loaded.clear();
                    self.group_note_edited_at.clear();
                    self.group_note_save_retry_at.clear();
                    self.group_note_saved_label.clear();
                    self.group_watch_inputs.clear();
                    self.group_watch_cache.clear();
                    self.group_stats_cache.clear();
                    self.script_list_cache.clear();
                    self.script_editing_id = None;
                    self.script_output.clear();
                    self.group_note_loading.clear();
                    self.group_note_load_error.clear();
                    self.group_stats_loading.clear();
                    self.group_summary_pending = None;
                    self.group_jump_pending = None;
                }
                GuiEvent::LoggedOut => {
                    self.is_logged_in = false;
                    self.user_name.clear();
                    self.user_uin.clear();
                    self.threads.clear();
                    if let Ok(mut watch) = VANISH_THREADS.lock() {
                        watch.clear();
                    }
                    self.selected_thread_index = 0;
                    self.qr_status = "Đang xin mã QR mới…".into();
                    self.status_msg = "Đã đăng xuất".into();
                    // Đăng xuất → cache nhóm không còn giá trị.
                    self.group_note_drafts.clear();
                    self.group_note_loaded.clear();
                    self.group_note_edited_at.clear();
                    self.group_note_save_retry_at.clear();
                    self.group_note_saved_label.clear();
                    self.group_watch_inputs.clear();
                    self.group_watch_cache.clear();
                    self.group_stats_cache.clear();
                    self.script_list_cache.clear();
                    self.script_editing_id = None;
                    self.script_output.clear();
                    self.group_note_loading.clear();
                    self.group_note_load_error.clear();
                    self.group_stats_loading.clear();
                    self.group_summary_pending = None;
                    self.group_jump_pending = None;
                }
                GuiEvent::AccountSwitched {
                    uin,
                    name,
                    avatar,
                    threads,
                } => {
                    // Worker đã snapshot DB acc cũ trước khi switch — flush pending
                    // ở đây phòng trường hợp GUI còn draft treo chưa kịp lưu.
                    self.flush_group_note_pending();
                    self.is_logged_in = true;
                    self.pending_add_account_uin = None;
                    self.user_uin = uin;
                    self.user_name = name.clone();
                    self.user_avatar = avatar;
                    self.threads = threads;
                    // Acc mới mang vanish policy riêng — rebuild watch để sweep
                    // thread khôi phục từ SQLite, không giữ watch của acc cũ (#875).
                    self.refresh_vanish_watch();
                    self.selected_thread_index = 0;
                    self.status_msg = format!("Đã chuyển sang tài khoản {name}");
                    self.history_reached_start.clear();
                    // Group Workspace cache theo tài khoản cũ → xoá để nạp lại từ DB mới.
                    self.group_note_drafts.clear();
                    self.group_note_loaded.clear();
                    self.group_note_edited_at.clear();
                    self.group_note_save_retry_at.clear();
                    self.group_note_saved_label.clear();
                    self.group_watch_inputs.clear();
                    self.group_watch_cache.clear();
                    self.group_stats_cache.clear();
                    self.script_list_cache.clear();
                    self.script_editing_id = None;
                    self.script_output.clear();
                    self.group_note_loading.clear();
                    self.group_note_load_error.clear();
                    self.group_stats_loading.clear();
                    self.group_summary_pending = None;
                    self.group_jump_pending = None;
                    // Nháp + reminder theo acc cũ → xoá để nạp lại từ DB mới.
                    self.draft_map.clear();
                    self.draft_reply_map.clear();
                    self.draft_threads.clear();
                    self.reminder_items.clear();
                    self.reminder_due_pending.clear();
                    self.remind_pick_msg = None;
                    self.remind_custom_open = false;
                    self.remind_custom_input.clear();
                    self.input_message.clear();
                    self.replying_to = None;
                    if let Some(tx) = WORKER_TX.get() {
                        let _ = tx.send(WorkerCmd::RefreshThreads);
                    }
                }
                GuiEvent::MessageSent {
                    client_message_id,
                    thread_id,
                    text,
                    msg_id,
                } => {
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        t.preview = text.clone();
                        let now = crate::ui::clock();
                        t.time = now.clone();
                        let final_id = if msg_id.is_empty() {
                            format!("out_{client_message_id}")
                        } else {
                            msg_id
                        };
                        merge_sent_message(
                            t,
                            ChatMessage {
                                id: final_id,
                                client_message_id,
                                sender: "Tôi".into(),
                                text,
                                time: now,
                                is_outgoing: true,
                                ts: (crate::login::unix_now() * 1000) as i64,
                                ..Default::default()
                            },
                        );
                        self.persist_threads_async();
                    }
                    // Gửi xong → xoá nháp thread này (RAM + SQLite).
                    self.draft_map.remove(&thread_id);
                    self.draft_reply_map.remove(&thread_id);
                    self.draft_threads.remove(&thread_id);
                    {
                        let db = crate::store::db_path_for_account(&self.user_uin);
                        if let Ok(mut st) = crate::store::MessageStore::open(&db) {
                            let _ = st.delete_draft(&thread_id);
                        }
                    }
                    self.status_msg = format!("Đã gửi tin nhắn lúc {}", crate::ui::clock());
                }
                GuiEvent::StickerSent {
                    client_message_id,
                    thread_id,
                    msg_id,
                    sticker_id,
                    cate_id,
                    sticker_type,
                } => {
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        t.preview = "[Nhãn dán]".into();
                        let now = crate::ui::clock();
                        t.time = now.clone();
                        let final_id = if msg_id.is_empty() {
                            format!("out_{client_message_id}")
                        } else {
                            msg_id
                        };
                        merge_sent_message(
                            t,
                            ChatMessage {
                                id: final_id,
                                client_message_id,
                                sender: "Tôi".into(),
                                text: "[Nhãn dán]".into(),
                                time: now,
                                is_outgoing: true,
                                ts: (crate::login::unix_now() * 1000) as i64,
                                sticker_id: Some(sticker_id),
                                sticker_cate_id: Some(cate_id),
                                sticker_type: Some(sticker_type),
                                ..Default::default()
                            },
                        );
                        self.persist_threads_async();
                    }
                    self.status_msg = format!("Đã gửi nhãn dán lúc {}", crate::ui::clock());
                }
                GuiEvent::AttachmentSent { thread_id, msg } => {
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        t.preview = msg.text.clone();
                        t.time = msg.time.clone();
                        merge_sent_message(t, msg);
                        self.persist_threads_async();
                    }
                    self.status_msg = format!("Đã gửi đính kèm lúc {}", crate::ui::clock());
                }
                GuiEvent::QuoteSent {
                    client_message_id,
                    thread_id,
                    text,
                    msg_id,
                    quoted,
                } => {
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        t.preview = text.clone();
                        let now = crate::ui::clock();
                        t.time = now.clone();
                        let final_id = if msg_id.is_empty() {
                            format!("out_{client_message_id}")
                        } else {
                            msg_id
                        };
                        merge_sent_message(
                            t,
                            ChatMessage {
                                id: final_id,
                                client_message_id,
                                sender: "Tôi".into(),
                                text,
                                time: now,
                                is_outgoing: true,
                                ts: (crate::login::unix_now() * 1000) as i64,
                                reply_to_id: Some(quoted.id.clone()),
                                reply_to_sender: Some(quoted.sender.clone()),
                                reply_to_text: Some(quoted.text.clone()),
                                ..Default::default()
                            },
                        );
                        self.persist_threads_async();
                    }
                    self.draft_map.remove(&thread_id);
                    self.draft_reply_map.remove(&thread_id);
                    self.draft_threads.remove(&thread_id);
                    {
                        let db = crate::store::db_path_for_account(&self.user_uin);
                        if let Ok(mut st) = crate::store::MessageStore::open(&db) {
                            let _ = st.delete_draft(&thread_id);
                        }
                    }
                    self.status_msg = format!("Đã gửi trả lời lúc {}", crate::ui::clock());
                }
                GuiEvent::Reacted { .. } => {
                    // Counts come only from the `chat.reaction` echo (ReactionEventsReceived);
                    // adding here as well double-counted every click.
                    self.status_msg = "Đã thả cảm xúc".into();
                }
                GuiEvent::ReactionEventsReceived(events) => {
                    // Count/banner ONLY newly accepted events (replays ignored) and
                    // only onto MY OWN messages (peer→peer reactions excluded).
                    // Target lookup is ID-first, timestamp-fallback (#1745); the
                    // per-thread unread loop additionally requires `is_outgoing`
                    // like the banner gate (#1812), and both loops iterate the
                    // accepted subset so repeats never double-count (#473, #1773).
                    let total = events.len();
                    let pending_before = self.reaction_inbox.pending_len();
                    let accepted = self.reaction_inbox.record_new(&events);
                    let mut changed = false;
                    for t in self.threads.iter_mut() {
                        changed |= self.reaction_inbox.apply_to(&mut t.messages);
                    }
                    crate::reactions::trace_batch(
                        total,
                        total.saturating_sub(
                            self.reaction_inbox
                                .pending_len()
                                .saturating_sub(pending_before),
                        ),
                        usize::from(changed),
                        &[],
                    );
                    if changed {
                        self.persist_threads_async();
                    }
                    // Reaction banners for peer reactions onto MY OWN messages
                    // (gate: skip own/removal/historical; respect mute + master toggle).
                    let mut reaction_banners: Vec<(String, String)> = Vec::new();
                    let my_uid = self.user_uin.clone();
                    let reactions_notify_on = self.settings.enable_notifications;
                    let reactions_sound_on =
                        self.settings.enable_notifications && self.settings.play_notification_sound;
                    let now_ts_react = crate::login::unix_now();
                    for ev in &accepted {
                        if ev.r_type < 0 || ev.historical {
                            continue;
                        }
                        if ev.sender_id.is_empty() || ev.sender_id == my_uid {
                            continue;
                        }
                        for t in &self.threads {
                            let muted = self
                                .settings
                                .muted_threads
                                .get(&t.id)
                                .copied()
                                .filter(|&u| u == -1 || u > now_ts_react)
                                .is_some()
                                || t.presentation.muted
                                || t.unread.is_muted();
                            if muted || !reactions_notify_on {
                                continue;
                            }
                            let hit = find_reaction_target(&t.messages, ev);
                            let Some(m) = hit else { continue };
                            if !m.is_outgoing {
                                continue;
                            }
                            let reactor = resolve_reactor_name(t, &ev.sender_id);
                            let body = crate::os_notify::reaction_banner_body(
                                &reactor,
                                &ev.emoji,
                                &m.text,
                            );
                            reaction_banners.push((t.name.clone(), body));
                            break;
                        }
                    }
                    // Per-thread reaction unread counts (in-memory only; summaries persist).
                    // Gate mirrors the banner block + focus/active-thread skip.
                    let active_tid_now = self
                        .threads
                        .get(self.selected_thread_index)
                        .map(|t| t.id.clone())
                        .unwrap_or_default();
                    let is_focused_now = ctx.input(|i| i.focused);
                    let my_uid_count = self.user_uin.clone();
                    for ev in &accepted {
                        if ev.r_type < 0 || ev.historical {
                            continue;
                        }
                        if ev.sender_id.is_empty() || ev.sender_id == my_uid_count {
                            continue;
                        }
                        let mut hit_tid: Option<String> = None;
                        for t in &self.threads {
                            let Some(m) = find_reaction_target(&t.messages, ev) else {
                                continue;
                            };
                            if !m.is_outgoing {
                                continue;
                            }
                            if !ev.thread_id.is_empty() {
                                hit_tid = Some(ev.thread_id.clone());
                                break;
                            } else {
                                hit_tid = Some(t.id.clone());
                                break;
                            }
                        }
                        let Some(tid) = hit_tid else { continue };
                        let is_muted = self
                            .settings
                            .muted_threads
                            .get(&tid)
                            .copied()
                            .filter(|&u| u == -1 || u > now_ts_react)
                            .is_some()
                            || self
                                .threads
                                .iter()
                                .find(|t| t.id == tid)
                                .map(|t| t.presentation.muted || t.unread.is_muted())
                                .unwrap_or(false);
                        if is_muted {
                            continue;
                        }
                        if tid == active_tid_now && is_focused_now {
                            continue;
                        }
                        *self.reaction_unread_counts.entry(tid).or_insert(0) += 1;
                    }
                    if !reaction_banners.is_empty() && reactions_notify_on {
                        for (title, body) in &reaction_banners {
                            crate::os_notify::notify(title, body);
                        }
                        if reactions_sound_on {
                            crate::os_notify::play_notification_sound();
                        }
                    }
                }
                GuiEvent::MessageDeleted {
                    thread_id,
                    msg_id,
                    ts,
                } => {
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        t.messages.retain(|m| {
                            !((!msg_id.is_empty() && m.id == msg_id)
                                || (msg_id.is_empty() && m.ts == ts))
                        });
                        if let Some(last) = t.messages.last() {
                            t.preview = last.text.clone();
                        }
                        self.persist_threads_async();
                    }
                    self.status_msg = "Đã xóa tin nhắn ở phía bạn".into();
                }
                GuiEvent::MessageRecalled {
                    thread_id,
                    msg_id,
                    ts,
                } => {
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        if let Some(m) = t.messages.iter_mut().find(|m| {
                            (!msg_id.is_empty() && m.id == msg_id)
                                || (msg_id.is_empty() && m.ts == ts)
                        }) {
                            if !m.text.starts_with("[Đã thu hồi]") {
                                if m.text.trim().is_empty() {
                                    m.text = "[Tin nhắn đã thu hồi]".into();
                                } else {
                                    m.text = format!("[Đã thu hồi] {}", m.text);
                                }
                            }
                            // Anti-recall: Preserve sticker_id, link_url, link_thumb so user can still see media!
                        }
                        if let Some(last) = t.messages.last() {
                            t.preview = last.text.clone();
                        }
                        self.persist_threads_async();
                    }
                    self.status_msg = "Có tin nhắn vừa bị thu hồi (đã lưu lại)".into();
                }
                GuiEvent::MessageEdited {
                    thread_id,
                    old_msg_id,
                    old_text,
                    new_text,
                    new_msg_id,
                    new_ts,
                } => {
                    // Sửa tin xong (WaveA3): thay text ngay tại chỗ, gắn cờ edited,
                    // lưu bản cũ vào SQLite. Hạn chế đã nói rõ: bên kia thấy
                    // thu hồi + tin mới (không phải edit liền mạch).
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        if let Some(m) = t.messages.iter_mut().find(|m| {
                            (!old_msg_id.is_empty() && m.id == old_msg_id)
                                || (old_msg_id.is_empty() && m.ts == new_ts)
                        }) {
                            m.text = new_text.clone();
                            m.edited = true;
                            m.id = new_msg_id.clone();
                            m.ts = new_ts;
                        }
                        if let Some(last) = t.messages.last() {
                            t.preview = last.text.clone();
                        }
                        self.persist_threads_async();
                    }
                    // Lưu bản cũ vào bảng message_edits (không block UI khi lỗi).
                    {
                        let db = crate::store::db_path_for_account(&self.user_uin);
                        if let Ok(mut st) = crate::store::MessageStore::open(&db) {
                            let _ = st.save_message_edit(&old_msg_id, &thread_id, &old_text, new_ts);
                        }
                    }
                    self.status_msg = format!("Đã sửa tin nhắn lúc {}", crate::ui::clock());
                }
                GuiEvent::VanishSettingChanged {
                    thread_id,
                    duration_sec,
                    started_at_sec,
                } => {
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        if duration_sec > 0 {
                            t.vanish = Some(crate::chat::VanishSetting {
                                duration_sec,
                                started_at_sec,
                            });
                        } else {
                            t.vanish = None;
                        }
                    }
                    if let Ok(mut watch) = VANISH_THREADS.lock() {
                        watch.retain(|id| *id != thread_id);
                        if duration_sec > 0 {
                            watch.push(thread_id);
                        }
                    }
                    if duration_sec > 0 {
                        self.status_msg = "⏳ Đã bật tin nhắn tự xoá cho hội thoại này".into();
                    } else {
                        self.status_msg = "Đã tắt tin nhắn tự xoá".into();
                    }
                }
                GuiEvent::VanishExpired {
                    thread_id,
                    expired_ids,
                } => {
                    if !expired_ids.is_empty() {
                        if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                            let mut flipped = false;
                            for m in t.messages.iter_mut() {
                                if expired_ids.contains(&m.id) {
                                    // `apply_vanish_expiry` clears link/sticker metadata
                                    // so `StoredMessage::from` yields empty raw_json +
                                    // tombstone text; `upsert_message` then overwrites
                                    // the SQLite row (non-empty tombstone marker) instead
                                    // of skipping it — the sweep ignores tombstones and
                                    // restart never restores original content (#1908).
                                    flipped |= crate::chat::apply_vanish_expiry(m);
                                }
                            }
                            if flipped {
                                if let Some(last) = t.messages.last() {
                                    t.preview = last.text.clone();
                                }
                                self.persist_threads_async();
                            }
                        }
                    }
                }
                GuiEvent::ForwardFinished { target_name, error } => match error {
                    None => {
                        self.show_toast(format!("Đã chuyển tiếp tới {target_name}"));
                        self.status_msg = format!("Đã chuyển tiếp tới {target_name}");
                    }
                    Some(err) => {
                        self.show_toast(format!("Chuyển tiếp tới {target_name} thất bại: {err}"));
                        self.status_msg = format!("Chuyển tiếp thất bại: {err}");
                    }
                },
                GuiEvent::ScreenshotCaptured {
                    path,
                    width,
                    height,
                    image,
                } => {
                    let tex = ctx.load_texture(
                        format!("clip_preview_{}", crate::login::unix_now()),
                        image,
                        TextureOptions::LINEAR,
                    );
                    self.pending_clipboard_image = Some((path, width, height, tex));
                    self.composer_focus_pending = true;
                    self.show_toast(format!(
                        "Đã chụp ảnh màn hình ({width}×{height} px) — nhấn Enter để gửi"
                    ));
                    self.status_msg = format!("Đã chụp ảnh màn hình ({width}×{height} px)");
                }
                GuiEvent::TypingEventsReceived(evs) => {
                    for ev in evs {
                        if ev.is_typing {
                            let name = if let Some(t) =
                                self.threads.iter().find(|t| t.id == ev.thread_id)
                            {
                                if t.is_group {
                                    self.group_members_map
                                        .get(&t.id)
                                        .and_then(|members| members.iter().find(|m| m.id == ev.uid))
                                        .map(|m| m.name.clone())
                                        .unwrap_or_else(|| {
                                            if ev.uid.is_empty() {
                                                "Thành viên".to_string()
                                            } else {
                                                format!("Người dùng {}", ev.uid)
                                            }
                                        })
                                } else {
                                    t.name.clone()
                                }
                            } else {
                                "Đối phương".to_string()
                            };
                            self.typing_map.insert(
                                ev.thread_id,
                                (
                                    name,
                                    std::time::Instant::now() + std::time::Duration::from_secs(6),
                                ),
                            );
                        } else {
                            self.typing_map.remove(&ev.thread_id);
                        }
                    }
                    ctx.request_repaint();
                }
                GuiEvent::SeenEventsReceived(evs) => {
                    for ev in evs {
                        if let Some(t) = self.threads.iter_mut().find(|t| t.id == ev.thread_id) {
                            for m in t.messages.iter_mut().filter(|m| m.is_outgoing) {
                                if ev.seen_ts == 0 || m.ts <= ev.seen_ts * 1000 + 5000 {
                                    m.delivery = crate::message_model::DeliveryState::Read;
                                }
                            }
                        }
                    }
                    ctx.request_repaint();
                }
                GuiEvent::StickerImageReady { key, image } => {
                    self.loading_stickers.remove(&key);
                    self.failed_stickers.remove(&key);
                    let tex = ctx.load_texture(&key, image, TextureOptions::LINEAR);
                    self.sticker_textures.insert(key, tex);
                }
                GuiEvent::StickerImageFailed { key } => {
                    self.loading_stickers.remove(&key);
                    if key.starts_with("call_remote_video") {
                        self.sticker_textures.remove(&key);
                        ctx.request_repaint();
                    } else {
                        record_sticker_fetch_failure(
                            &mut self.failed_stickers,
                            &key,
                            std::time::Instant::now(),
                        );
                    }
                }
                GuiEvent::StickerStoreLoaded(packs) => {
                    self.sticker_store_loading = false;
                    self.sticker_store_attempted = true;
                    let count = packs.len();
                    self.sticker_store_packs = packs;
                    if count > 0 {
                        self.status_msg = format!("Đã tải Kho Nhãn Dán Zalo ({count} bộ nhãn)");
                    } else {
                        self.status_msg = "Chưa tải được Kho Nhãn Dán Zalo (thử bấm Tải lại kho)".into();
                    }
                }
                GuiEvent::StickerPackItemsLoaded {
                    cate_id,
                    name,
                    items,
                } => {
                    self.loading_sticker_packs.remove(&cate_id);
                    let count = items.len();
                    if items.is_empty() {
                        self.failed_sticker_packs.insert(cate_id);
                        self.status_msg = format!("Chưa tải được bộ nhãn \"{name}\"");
                    } else {
                        self.failed_sticker_packs.remove(&cate_id);
                        if !self
                            .pinned_sticker_packs
                            .iter()
                            .any(|(id, _)| *id == cate_id)
                        {
                            self.pinned_sticker_packs.insert(0, (cate_id, name.clone()));
                        }
                        self.sticker_pack_items.insert(cate_id, items);
                        self.status_msg = format!("Đã mở bộ nhãn \"{name}\" ({count} nhãn dán)");
                    }
                }
                GuiEvent::StickerSearchResults { keyword, items } => {
                    self.sticker_search_loading = false;
                    let count = items.len();
                    self.sticker_search_results = items;
                    self.status_msg =
                        format!("Tìm thấy {count} nhãn dán cho từ khóa \"{keyword}\"");
                }
                GuiEvent::StickerSuggestKeywordsLoaded(kws) => {
                    for k in kws {
                        self.sticker_suggest_keywords.insert(k);
                    }
                }
                GuiEvent::ComposerStickerSuggestions { keyword, items } => {
                    self.composer_sticker_loading.remove(&keyword);
                    self.composer_sticker_cache.insert(keyword, items);
                }
                GuiEvent::LinkPreviewReady(meta) => {
                    self.loading_link_previews.remove(&meta.url);
                    if !meta.thumb.is_empty() {
                        let key = format!("thumb_{}", crate::crypto::md5_hex(&meta.thumb));
                        self.request_sticker_image(key, meta.thumb.clone());
                    }
                    let mut updated_any = false;
                    for t in &mut self.threads {
                        for m in &mut t.messages {
                            let matches_url = m.link_url.as_deref() == Some(meta.url.as_str())
                                || crate::chat::extract_first_url(&m.text).as_deref()
                                    == Some(meta.url.as_str());
                            if matches_url {
                                if m.link_url.is_none() {
                                    m.link_url = Some(meta.url.clone());
                                    updated_any = true;
                                }
                                if (m.link_title.is_none()
                                    || m.link_title.as_deref() == Some(meta.url.as_str()))
                                    && !meta.title.is_empty()
                                {
                                    m.link_title = Some(meta.title.clone());
                                    updated_any = true;
                                }
                                if m.link_desc.is_none() && !meta.desc.is_empty() {
                                    m.link_desc = Some(meta.desc.clone());
                                    updated_any = true;
                                }
                                let old_thumb_expired = m
                                    .link_thumb
                                    .as_deref()
                                    .map(|th| {
                                        th.is_empty()
                                            || (th.contains("zpcloud.zdn.vn")
                                                && !self.sticker_textures.contains_key(&format!(
                                                    "thumb_{}",
                                                    crate::crypto::md5_hex(th)
                                                )))
                                    })
                                    .unwrap_or(true);
                                if old_thumb_expired && !meta.thumb.is_empty() {
                                    m.link_thumb = Some(meta.thumb.clone());
                                    updated_any = true;
                                }
                                if m.link_src.is_none() && !meta.src.is_empty() {
                                    m.link_src = Some(meta.src.clone());
                                    updated_any = true;
                                }
                            }
                        }
                    }
                    self.link_metadata_cache.insert(meta.url.clone(), meta);
                    if updated_any {
                        self.persist_threads_async();
                    }
                }
                GuiEvent::MessageTranslated {
                    thread_id,
                    msg_id,
                    text,
                    generation,
                } => {
                    let key = crate::chat::translate_request_key(&thread_id, &msg_id);
                    // Bỏ completion cũ (user đã bấm Dịch lại với target khác).
                    if self.translate_generation.get(&key).copied().unwrap_or(0) != generation {
                        return;
                    }
                    self.translate_generation.remove(&key);
                    self.translate_pending.remove(&key);
                    if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                        if let Some(m) = t
                            .messages
                            .iter_mut()
                            .find(|m| crate::message_navigation::message_matches_reference(m, &msg_id))
                        {
                            // Từ chối kết quả tới sau khi tin đã vanish.
                            if crate::chat::is_vanished_message(m) {
                                return;
                            }
                            m.translation = Some(text);
                        }
                    }
                }
                GuiEvent::GroupMembersLoaded { group_id, members } => {
                    self.fetching_group_members.remove(&group_id);
                    for m in &members {
                        if let Some(ref ava_url) = m.avatar {
                            if ava_url.starts_with("http")
                                && !ava_url.ends_with("/default")
                                && !ava_url.contains("default_avatar")
                                && !ava_url.contains("avatar_default")
                            {
                                let file_or_base =
                                    crate::chat::avatar_file_key(Some(ava_url.as_str()))
                                        .unwrap_or_else(|| {
                                            ava_url.split('?').next().unwrap_or(ava_url).to_string()
                                        });
                                let key = format!("avt_{}", crate::crypto::md5_hex(&file_or_base));
                                self.request_sticker_image(key, ava_url.clone());
                            }
                        }
                    }
                    if let Some(t) = self.threads.iter_mut().find(|th| th.id == group_id) {
                        t.presentation.members = members.clone();
                        t.presentation.member_count = Some(members.len() as u64);
                    }
                    self.group_members_map.insert(group_id, members);
                }
                GuiEvent::FriendStatusChecked {
                    user_id,
                    status,
                    online,
                } => {
                    self.checking_friend_status.remove(&user_id);
                    self.friend_status_map.insert(user_id.clone(), status);
                    if let Some(onl) = online {
                        self.user_online_map.insert(user_id.clone(), onl);
                    }
                    if let Some(ref mut prof) = self.phone_lookup_result {
                        if prof.user_id == user_id {
                            prof.friendship = status;
                        }
                    }
                }
                GuiEvent::PhoneLookupSuccess(res) => {
                    self.phone_lookup_loading = false;
                    self.phone_lookup_error = None;
                    self.phone_modal_open = true;
                    self.show_toast(format!("Đã tìm thấy tài khoản: {}", res.display_name));
                    self.friend_status_map
                        .insert(res.user_id.clone(), res.friendship);
                    self.phone_lookup_result = Some(res);
                }
                GuiEvent::PhoneLookupFailed(err) => {
                    self.phone_lookup_loading = false;
                    self.phone_lookup_result = None;
                    self.phone_modal_open = true;
                    self.phone_lookup_error = Some(err);
                }
                GuiEvent::SendFailed {
                    thread_id: _,
                    text,
                    error,
                } => {
                    if self.input_message.is_empty() && !text.is_empty() {
                        self.input_message = text;
                    }
                    self.status_msg = format!("Không gửi được: {error}");
                }
                GuiEvent::CallStateChanged(state) => {
                    if state == "free" {
                        // Keep modal open if there is a rejection reason so the user can read it
                        let has_error = self
                            .active_call
                            .as_ref()
                            .and_then(|c| c.error_reason.as_ref())
                            .is_some();
                        self.sticker_textures.remove("call_remote_video");
                        self.sticker_textures.remove("call_remote_video_1");
                        self.sticker_textures.remove("call_remote_video_2");
                        self.sticker_textures.remove("call_remote_video_3");
                        self.sticker_textures.remove("call_remote_video_4");
                        self.sticker_textures.remove("call_remote_video_5");
                        self.sticker_textures.remove("call_local_video");
                        if !has_error {
                            self.active_call = None;
                        }
                    } else if let Some(ref mut call) = self.active_call {
                        if state == "incall" || state == "connected" {
                            call.is_incoming_ringing = false;
                        }
                        if call.error_reason.is_none() {
                            call.state_label = match state.as_str() {
                                "calling" | "ringing" => "Đang đổ chuông…".into(),
                                "incoming_ringing" => "Cuộc gọi đến đang đổ chuông…".into(),
                                "connecting" => "Đang kết nối luồng âm thanh/video…".into(),
                                "incall" | "connected" => {
                                    "Đang trong cuộc gọi (Pure-Rust VoIP)".into()
                                }
                                _ => format!("Đang kết nối ({state})…"),
                            };
                        }
                    }
                }
                GuiEvent::IncomingCall {
                    call_id: _,
                    caller_id,
                    caller_name,
                    is_video,
                    is_group,
                } => {
                    let resolved_name = self
                        .threads
                        .iter()
                        .find(|t| t.id == caller_id)
                        .map(|t| t.name.clone())
                        .unwrap_or(caller_name);
                    self.status_msg = format!(
                        "📲 Cuộc gọi {} đến từ {resolved_name}…",
                        if is_video { "video" } else { "thoại" }
                    );
                    self.active_call = Some(ActiveCallOverlay {
                        partner_id: caller_id,
                        partner_name: resolved_name,
                        is_video,
                        is_group,
                        state_label: "Cuộc gọi đến đang đổ chuông…".into(),
                        started_at: std::time::Instant::now(),
                        mic_muted: false,
                        cam_enabled: is_video,
                        is_incoming_ringing: true,
                        error_code: None,
                        error_reason: None,
                        speaker_view: false,
                        pinned_tile_idx: 0,
                        call_page: 0,
                        members_popover_open: false,
                        invite_search_query: String::new(),
                        quick_reply_popover_open: false,
                    });
                    ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                        egui::UserAttentionType::Critical,
                    ));
                }
                GuiEvent::CallRejected { code, reason } => {
                    self.status_msg = format!("⚠️ Từ chối cuộc gọi (mã {code}): {reason}");
                    if let Some(ref mut call) = self.active_call {
                        call.state_label = "Không thể thực hiện cuộc gọi".into();
                        call.error_code = Some(code);
                        call.error_reason = Some(reason);
                    }
                }
                GuiEvent::StatusUpdate(msg) => {
                    self.status_msg = msg;
                }
                GuiEvent::ScriptToast { message, .. } => {
                    self.show_toast(message);
                }
                GuiEvent::GroupLeft { group_id: _, success, error } => {
                    if success {
                        self.show_toast("Đã rời nhóm thành công");
                    } else if let Some(err) = error {
                        self.show_toast(format!("Rời nhóm thất bại: {err}"));
                    }
                }
                GuiEvent::ScriptLogsUpdated { .. } => {
                    // Drawer Scripts đọc log trực tiếp từ DB khi render nên chỉ cần repaint.
                    ctx.request_repaint();
                }
                GuiEvent::HistoryReachedStart(tid) => {
                    self.history_reached_start.insert(tid);
                    self.show_toast("Đã hiển thị toàn bộ lịch sử tin nhắn của cuộc trò chuyện này");
                }
                GuiEvent::GroupNoteLoaded { thread_id, uin, note, watch, error } => {
                    self.group_note_loading.remove(&thread_id);
                    // Stale (đổi acc trước khi worker trả) → bỏ, không đè cache acc mới.
                    if uin != self.user_uin {
                        return;
                    }
                    match note {
                        Some(n) => {
                            // Không đè draft user đang gõ: chỉ nạp khi chưa ai sửa.
                            if !self.group_note_edited_at.contains_key(&thread_id)
                                && !self.group_note_drafts.contains_key(&thread_id)
                            {
                                self.group_note_drafts.insert(thread_id.clone(), n);
                            }
                            self.group_watch_cache.insert(thread_id.clone(), watch);
                            self.group_note_loaded.insert(thread_id.clone());
                            self.group_note_load_error.remove(&thread_id);
                        }
                        None => {
                            self.group_note_load_error.insert(
                                thread_id.clone(),
                                error.unwrap_or_else(|| "Không mở được DB".to_string()),
                            );
                        }
                    }
                }
                GuiEvent::GroupNoteSaved { thread_id, saved_at_label, error } => {
                    match error {
                        None => {
                            self.group_note_saved_label.insert(thread_id.clone(), saved_at_label);
                            // Thành công mới clear pending; fail giữ lại để tick sau retry.
                            self.group_note_edited_at.remove(&thread_id);
                            self.group_note_save_retry_at.remove(&thread_id);
                        }
                        Some(e) => {
                            self.group_note_saved_label.insert(
                                thread_id.clone(),
                                "Lưu thất bại, sẽ thử lại".to_string(),
                            );
                            // Giữ group_note_edited_at (pending) + đánh dấu mốc retry.
                            self.group_note_save_retry_at
                                .insert(thread_id.clone(), std::time::Instant::now());
                            let _ = e;
                        }
                    }
                }
                GuiEvent::GroupStatsReady { thread_id, uin, stats } => {
                    self.group_stats_loading.remove(&thread_id);
                    if uin != self.user_uin {
                        return;
                    }
                    self.group_stats_cache
                        .insert(thread_id, (std::time::Instant::now(), stats));
                }
                GuiEvent::RefreshFinished {
                    threads,
                    messages,
                    errors,
                    manual,
                } => {
                    let msg = if errors.is_empty() {
                        format!("✅ Đã làm mới {threads} hội thoại ({messages} tin nhắn)")
                    } else {
                        format!(
                            "⚠️ Làm mới {threads} hội thoại · {} lỗi (vd. {})",
                            errors.len(),
                            errors[0]
                        )
                    };
                    self.status_msg = msg.clone();
                    if manual {
                        self.show_toast(msg);
                    }
                }
                GuiEvent::ScheduledListUpdated => {
                    let prev_cnt = self.scheduled_items.len();
                    self.refresh_scheduled_items();
                    if self.scheduled_items.len() < prev_cnt {
                        if self.settings.enable_notifications
                            && self.settings.play_notification_sound
                        {
                            crate::media::play_notification_chime();
                        }
                        self.show_toast("⏰ Đã gửi tự động tin nhắn hẹn giờ / nhắc hẹn");
                    }
                }
                GuiEvent::ReminderDue(items) => {
                    if items.is_empty() {
                        return;
                    }
                    if self.settings.enable_notifications && self.settings.play_notification_sound {
                        crate::media::play_notification_chime();
                    }
                    for r in &items {
                        if !self.reminder_due_pending.iter().any(|p| {
                            p.thread_id == r.thread_id && p.msg_id == r.msg_id && p.due_at == r.due_at
                        }) {
                            self.reminder_due_pending.push(r.clone());
                        }
                    }
                    self.refresh_draft_reminder_cache();
                    let first = &items[0];
                    let preview: String = first.text_preview.chars().take(80).collect();
                    let extra = if items.len() > 1 {
                        format!(" (+{} nhắc khác)", items.len() - 1)
                    } else {
                        String::new()
                    };
                    self.show_toast(format!("⏰ Nhắc tin: {preview}{extra} — bấm vào đây để xem"));
                }
                GuiEvent::ReminderListUpdated => {
                    self.refresh_draft_reminder_cache();
                }
                GuiEvent::SyncFinished {
                    added,
                    errors,
                    cancelled,
                } => {
                    let head = if cancelled {
                        "Đã dừng đồng bộ"
                    } else {
                        "Đồng bộ xong"
                    };
                    let mut msg = format!("{head}: lưu thêm {added} tin cũ");
                    if let Some(first) = errors.first() {
                        msg.push_str(&format!(" · {} lỗi (vd. {first})", errors.len()));
                    }
                    self.status_msg = msg.clone();
                    self.show_toast(msg);
                }
                GuiEvent::UpdateCheckFinished { result, silent } => {
                    self.update_checking = false;
                    match result {
                        Ok(info) => {
                            if info.has_update {
                                let msg = format!(
                                    "🚀 Có bản cập nhật mới v{} trên GitHub (hiện tại: v{})",
                                    info.latest_version, info.current_version
                                );
                                self.update_status = msg.clone();
                                let is_skipped = silent
                                    && !self.settings.skipped_update_version.is_empty()
                                    && self.settings.skipped_update_version == info.latest_version;
                                let already_notified = self.last_notified_update_version.as_deref()
                                    == Some(info.latest_version.as_str());
                                if !is_skipped {
                                    self.show_toast(msg);
                                    if !already_notified || !silent {
                                        self.last_notified_update_version =
                                            Some(info.latest_version.clone());
                                        self.update_modal_open = true;
                                        self.update_banner_dismissed = false;
                                        if self.settings.enable_notifications {
                                            crate::updater::notify_os_new_update(
                                                &info.latest_version,
                                                &info.current_version,
                                                &info.release_title,
                                            );
                                        }
                                    }
                                }
                            } else {
                                self.update_status = format!(
                                    "✅ Bạn đang sử dụng phiên bản mới nhất (v{})",
                                    info.current_version
                                );
                                if !silent {
                                    self.show_toast(&self.update_status.clone());
                                }
                            }
                            self.update_info = Some(info);
                        }
                        Err(e) => {
                            self.update_status = format!("⚠️ Không kiểm tra được cập nhật: {e}");
                            if !silent {
                                self.show_toast(&self.update_status.clone());
                            }
                        }
                    }
                }
                GuiEvent::UpdateDownloadStatus {
                    downloading,
                    message,
                } => {
                    self.update_downloading = downloading;
                    if !downloading {
                        self.update_progress = None;
                    }
                    self.update_status = message.clone();
                    self.show_toast(message);
                }
                GuiEvent::UpdateDownloadProgress { downloaded, total } => {
                    self.update_downloading = true;
                    self.update_progress = Some((downloaded, total));
                }
                GuiEvent::UpdateAppliedAndRestarting(ver) => {
                    self.update_downloading = false;
                    self.show_toast(format!("✅ Đã cập nhật lên v{ver} — Đang khởi động lại…"));
                    self.allow_quit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                GuiEvent::SocialProfileLoaded(res) => {
                    self.feed_loading_profile = false;
                    match res {
                        Ok(prof) => {
                            self.feed_profile = Some(prof);
                            self.feed_profile_error = None;
                        }
                        Err(e) => {
                            self.feed_profile_error = Some(e);
                        }
                    }
                }
                GuiEvent::SocialAvatarsLoaded(res) => {
                    self.feed_loading_avatars = false;
                    match res {
                        Ok(photos) => {
                            self.feed_avatars = photos;
                            self.feed_avatars_error = None;
                        }
                        Err(e) => {
                            self.feed_avatars_error = Some(e);
                        }
                    }
                }
                GuiEvent::SocialTimelineLoaded(res) => {
                    self.feed_loading_timeline = false;
                    match res {
                        Ok((profiles, items)) => {
                            self.feed_friend_profiles = profiles;
                            self.feed_items = items;
                        }
                        Err(e) => {
                            self.feed_profile_error = Some(e);
                        }
                    }
                }
                GuiEvent::MuteSnapshotSynced(mute_map) => {
                    self.settings.muted_threads = mute_map.clone();
                    self.settings.save();
                    let now_ts = crate::login::unix_now() as i64;
                    for t in &mut self.threads {
                        if let Some(&until) = mute_map.get(&t.id) {
                            let active = until == -1 || until > now_ts;
                            t.presentation.muted = active;
                            t.unread.muted = active;
                            t.unread.muted_until = (until > 0).then_some(until);
                            t.unread.settings_fresh = true;
                            t.unread.fresh = true;
                        } else {
                            t.presentation.muted = false;
                            t.unread.muted = false;
                            t.unread.muted_until = None;
                            t.unread.settings_fresh = true;
                            t.unread.fresh = true;
                        }
                    }
                    self.persist_threads_async();
                }
                GuiEvent::PinnedSnapshotSynced(pinned_ids) => {
                    for pid in &pinned_ids {
                        let clean = pid
                            .trim_start_matches("group_")
                            .trim_start_matches('g')
                            .trim_start_matches('G')
                            .trim_start_matches('u')
                            .trim_start_matches('U')
                            .trim()
                            .to_string();
                        if !clean.is_empty() && !self.settings.pinned_threads.contains(&clean) {
                            self.settings.pinned_threads.push(clean);
                        }
                    }
                    crate::chat::apply_pinned_threads(&mut self.threads, &pinned_ids);
                    self.settings.save();
                    self.persist_threads_async();
                }
                GuiEvent::MuteEventsReceived(events) => {
                    let mut changed = false;
                    for ev in events {
                        if ev.muted {
                            let until_val = ev.muted_until.unwrap_or(-1);
                            self.settings
                                .muted_threads
                                .insert(ev.thread_id.clone(), until_val);
                        } else {
                            self.settings.muted_threads.remove(&ev.thread_id);
                        }
                        if let Some(t) = self.threads.iter_mut().find(|t| t.id == ev.thread_id) {
                            t.presentation.muted = ev.muted;
                            t.unread.muted = ev.muted;
                            t.unread.muted_until = ev.muted_until;
                            t.unread.settings_fresh = true;
                            t.unread.fresh = true;
                        }
                        changed = true;
                    }
                    if changed {
                        self.settings.save();
                        self.persist_threads_async();
                    }
                }
                GuiEvent::PhoneSyncProgress { progress, status } => {
                    self.phone_sync_progress = progress;
                    self.phone_sync_status = status.clone();
                    self.status_msg = status;
                }
                GuiEvent::PhoneSyncFinished {
                    threads,
                    added,
                    active_threads,
                    cancelled,
                    error,
                } => {
                    self.phone_sync_progress = 0.0;
                    if !threads.is_empty() {
                        for nt in threads {
                            let is_pinned = nt.presentation.pinned
                                || self.settings.pinned_threads.contains(&nt.id)
                                || crate::chat::is_pinned_in_list(
                                    &nt.id,
                                    nt.is_group,
                                    &self.settings.pinned_threads,
                                );
                            if is_pinned {
                                let clean = nt
                                    .id
                                    .trim_start_matches("group_")
                                    .trim_start_matches('g')
                                    .trim_start_matches('G')
                                    .trim_start_matches('u')
                                    .trim_start_matches('U')
                                    .trim()
                                    .to_string();
                                if !clean.is_empty()
                                    && !self.settings.pinned_threads.contains(&clean)
                                {
                                    self.settings.pinned_threads.push(clean);
                                }
                            }
                            if let Some(existing) = self.threads.iter_mut().find(|t| t.id == nt.id)
                            {
                                crate::chat::merge_thread_into(existing, nt, true);
                                if is_pinned {
                                    existing.presentation.pinned = true;
                                }
                            } else if !nt.id.is_empty() && nt.id != "0" {
                                self.threads.push(nt);
                            }
                        }
                        self.settings.save();
                        self.persist_threads_async();
                    }
                    let msg = phone_sync_completion_message(
                        added,
                        active_threads,
                        cancelled,
                        error.as_deref(),
                    );
                    self.phone_sync_status = msg.clone();
                    self.status_msg = msg.clone();
                    self.show_toast(msg);
                }
                GuiEvent::MessagePinResult {
                    thread_id,
                    msg_id,
                    pin,
                    topic_id,
                    error,
                } => {
                    if let Some(err) = error {
                        // The server refused the board call: roll the optimistic flip back.
                        if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                            if let Some(m) = t.messages.iter_mut().find(|m| m.id == msg_id) {
                                m.is_pinned = !pin;
                                if !pin {
                                    m.board_topic_id.clear();
                                }
                            }
                        }
                        let msg = format!("⚠️ Không ghim được tin nhắn: {err}");
                        self.status_msg = msg.clone();
                        self.show_toast(msg);
                    } else if !topic_id.is_empty() {
                        if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                            if let Some(m) = t.messages.iter_mut().find(|m| m.id == msg_id) {
                                m.board_topic_id = topic_id;
                            }
                        }
                        self.persist_threads_async();
                    }
                }
                GuiEvent::BoardPinsUpdated {
                    thread_id,
                    topics,
                    error,
                } => {
                    if let Some(_err) = error {
                        // Board read-back is parked (the list API rejects every shape we can build),
                        // so stay completely silent: a toast on each thread switch is unacceptable.
                    } else {
                        // The server is authoritative: match each topic back to its bubble and drop
                        // stale local pin flags inside this thread.
                        let pinned_ids: Vec<String> =
                            topics.iter().map(|t| t.id.clone()).collect();
                        if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                            let mut changed = false;
                            for m in t.messages.iter_mut() {
                                let mut found: Option<&crate::chat::BoardTopic> = None;
                                for topic in &topics {
                                    if topic.msg_ids.iter().any(|id| {
                                        id == &m.id
                                            || (!m.client_message_id.is_empty()
                                                && id == &m.client_message_id)
                                    }) {
                                        found = Some(topic);
                                        break;
                                    }
                                }
                                match found {
                                    Some(topic) => {
                                        if !m.is_pinned || m.board_topic_id != topic.id {
                                            m.is_pinned = true;
                                            m.board_topic_id = topic.id.clone();
                                            changed = true;
                                        }
                                    }
                                    None => {
                                        if m.is_pinned && !pinned_ids.contains(&m.board_topic_id) {
                                            m.is_pinned = false;
                                            m.board_topic_id.clear();
                                            changed = true;
                                        }
                                    }
                                }
                            }
                            if changed {
                                self.persist_threads_async();
                            }
                        }
                    }
                }
                GuiEvent::ProxyUpdated(res) => match res {
                    Ok(Some(url)) => {
                        let redacted = crate::session::redact_proxy_url(&url);
                        self.settings.proxy_url = url;
                        self.settings.save();
                        let msg = format!("🌐 Đã áp dụng Proxy: {redacted}");
                        self.status_msg = msg.clone();
                        self.show_toast(msg);
                    }
                    Ok(None) => {
                        self.settings.proxy_url.clear();
                        self.settings.save();
                        let msg = "🌐 Đã tắt Proxy (kết nối trực tiếp)".to_string();
                        self.status_msg = msg.clone();
                        self.show_toast(msg);
                    }
                    Err(e) => {
                        let msg = format!("⚠️ Lỗi cấu hình Proxy: {e}");
                        self.status_msg = msg.clone();
                        self.show_toast(msg);
                    }
                },
                GuiEvent::FileDownloaded(res) => match res {
                    Ok((fname, path)) => {
                        let msg = format!("✅ Đã lưu tập tin \"{fname}\" tại {path}");
                        self.status_msg = msg.clone();
                        self.show_toast(msg);
                        // Mở thư mục chứa file để user khỏi mất công tìm (#102).
                        reveal_in_file_manager(&path);
                    }
                    Err(e) => {
                        let msg = format!("⚠️ Lỗi lưu tập tin: {e}");
                        self.status_msg = msg.clone();
                        self.show_toast(msg);
                    }
                },
                GuiEvent::DocPreviewReady { msg_id, text } => {
                    self.doc_preview_loading.remove(&msg_id);
                    self.doc_preview_errors.remove(&msg_id);
                    self.doc_previews.insert(msg_id, text);
                }
                GuiEvent::DocPreviewFailed { msg_id, err } => {
                    self.doc_preview_loading.remove(&msg_id);
                    self.doc_preview_errors.insert(msg_id, err);
                }
                GuiEvent::AttachmentPickerCancelled { thread_id, caption } => {
                    if !caption.is_empty() {
                        let active_tid = self
                            .threads
                            .get(self.selected_thread_index)
                            .map(|t| t.id.as_str())
                            .unwrap_or("");
                        if active_tid == thread_id && self.input_message.trim().is_empty() {
                            self.input_message = caption;
                            self.composer_focus_pending = true;
                        } else {
                            self.draft_map.entry(thread_id.clone()).or_insert(caption);
                            self.draft_threads.insert(thread_id);
                        }
                    }
                }
                GuiEvent::GroupSummaryReady { message } => {
                    self.show_toast(message);
                }
                GuiEvent::ThreadScriptsLoaded {
                    thread_id,
                    scripts,
                    logs,
                } => {
                    self.script_loading_threads.remove(&thread_id);
                    self.script_list_cache.insert(thread_id.clone(), scripts);
                    self.script_logs_cache.insert(thread_id, logs);
                }
                GuiEvent::ThreadScriptSaved {
                    thread_id,
                    close_editor,
                    ok,
                    toast,
                    logs,
                } => {
                    self.script_loading_threads.remove(&thread_id);
                    self.script_list_cache.remove(&thread_id);
                    if let Some(updated_logs) = logs {
                        self.script_logs_cache.insert(thread_id, updated_logs);
                    }
                    if ok && close_editor {
                        self.script_tab = 0;
                        self.script_editing_id = None;
                    }
                    if let Some(msg) = toast {
                        self.show_toast(msg);
                    }
                }
                GuiEvent::ThreadScriptRunResult {
                    thread_id,
                    output,
                    outbox,
                    toasts,
                    is_group,
                    logs,
                } => {
                    for o in &outbox {
                        if let Some(wtx) = WORKER_TX.get() {
                            let _ = wtx.send(WorkerCmd::SendMessage {
                                to_id: o.thread_id.clone(),
                                text: o.text.clone(),
                                is_group,
                            });
                        }
                    }
                    for t in toasts {
                        self.show_toast(t);
                    }
                    self.script_output = output;
                    self.script_logs_cache.insert(thread_id, logs);
                }
            }
        }
    }
}

/// Hiện file trong trình quản lý file của OS (Finder/Explorer/xdg-open cha).
/// Fire-and-forget: lỗi mở bị bỏ qua, không bao giờ panic UI (#102).
fn reveal_in_file_manager(path: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").args(["-R", path]).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer")
            .arg(format!("/select,{path}"))
            .spawn();
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let parent = std::path::Path::new(path)
            .parent()
            .map(|p| p.as_os_str())
            .unwrap_or(std::ffi::OsStr::new("."));
        let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
    }
}

// ---------------------------------------------------------------------------
// Lightweight Background Session Worker (2-thread Tokio pool, off-UI disk I/O)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Main eframe::App Implementation
// ---------------------------------------------------------------------------

impl eframe::App for ZaloGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ensure_windows_frameless_subclass(ctx);

        if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::Q))
            || ctx.input(|i| i.viewport().close_requested())
        {
            self.allow_quit = true;
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::EndCall);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if ctx.input(|i| {
            (i.modifiers.ctrl || i.modifiers.command) && i.modifiers.shift && i.key_pressed(Key::P)
        }) {
            self.settings_modal_open = true;
            self.settings_tab = SettingsTab::Privacy;
        }
        if self.settings.theme_mode == "system" {
            let os_dark = resolve_dark_mode("system", ctx);
            if os_dark != self.dark_mode {
                self.dark_mode = os_dark;
                self.theme_dirty = true;
            }
        }
        let expected_egui_theme = if self.dark_mode {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        };
        if ctx.theme() != expected_egui_theme
            || ctx.style().visuals.dark_mode != self.dark_mode
        {
            self.theme_dirty = true;
        }
        if self.theme_dirty {
            self.refresh_cached_palette();
            self.cached_palette.apply_to_ctx_with_anim(
                ctx,
                self.settings.animations_enabled,
                self.settings.normalized_animation_speed(),
            );
            self.theme_dirty = false;
        }

        self.check_events(ctx);

        self.ime_just_committed = false;
        ctx.input_mut(|i| {
            let mut ime_commit_in_frame = false;
            let mut ime_space_commit = false;

            #[cfg(target_os = "macos")]
            let space_is_down = {
                #[link(name = "CoreGraphics", kind = "framework")]
                extern "C" {
                    fn CGEventSourceKeyState(state_id: i32, key: u16) -> bool;
                }
                // state_id = 0 (kCGEventSourceStateCombinedSessionState), key = 49 (kVK_Space)
                unsafe { CGEventSourceKeyState(0, 49) }
            };
            #[cfg(not(target_os = "macos"))]
            let space_is_down = false;

            let space_in_events = i.events.iter().any(|e| match e {
                egui::Event::Key {
                    key: Key::Space,
                    pressed: true,
                    ..
                } => true,
                egui::Event::Text(s) if s == " " => true,
                _ => false,
            }) || i.key_down(Key::Space);

            // Check whether Enter, non-space punctuation/navigation, or a mouse click
            // occurred in the same frame so we only auto-append a trailing space when
            // Spacebar triggered the macOS Vietnamese Telex IME commit (#71).
            let enter_pressed = i.events.iter().any(|e| {
                matches!(
                    e,
                    egui::Event::Key {
                        key: Key::Enter,
                        pressed: true,
                        ..
                    }
                )
            });
            let non_space_trigger = i.events.iter().any(|e| match e {
                egui::Event::Text(s) => s != " ",
                egui::Event::Key {
                    key, pressed: true, ..
                } => !matches!(key, Key::Space | Key::Backspace),
                _ => false,
            });
            let mouse_clicked =
                i.pointer.any_pressed() || i.pointer.any_released() || i.pointer.any_click();
            let space_pressed = space_is_down || space_in_events;
            let mut was_composing = self.ime_active;

            for event in &mut i.events {
                match event {
                    egui::Event::Ime(egui::ImeEvent::Enabled) => {
                        self.ime_active = true;
                        was_composing = true;
                    }
                    egui::Event::Ime(egui::ImeEvent::Preedit(text)) => {
                        self.ime_active = !text.is_empty();
                        if !text.is_empty() {
                            was_composing = true;
                        }
                    }
                    egui::Event::Ime(egui::ImeEvent::Commit(ref mut text)) => {
                        let inferred_macos_space_commit = cfg!(target_os = "macos")
                            && was_composing
                            && !enter_pressed
                            && !non_space_trigger
                            && !mouse_clicked;
                        if (space_pressed || inferred_macos_space_commit)
                            && !text.is_empty()
                            && !text.chars().any(crate::gui::theme::is_cjk)
                        {
                            if !text.ends_with(' ') {
                                text.push(' ');
                            }
                            ime_space_commit = true;
                        }
                        self.ime_active = false;
                        self.ime_just_committed = true;
                        ime_commit_in_frame = true;
                    }
                    egui::Event::Ime(egui::ImeEvent::Disabled) => {
                        self.ime_active = false;
                    }
                    _ => {}
                }
            }

            if ime_space_commit {
                // Prevent duplicate space insertion if winit also forwarded a Space event in the same frame.
                i.events.retain(|e| {
                    !matches!(e, egui::Event::Text(s) if s == " ")
                        && !matches!(
                            e,
                            egui::Event::Key {
                                key: Key::Space,
                                pressed: true,
                                ..
                            }
                        )
                });
            }

            if self.ime_active || ime_commit_in_frame {
                // Suppress Enter key events during IME composition/commit so pressing Enter
                // to accept an IME candidate does not blur TextEdit or send prematurely (#71).
                i.events.retain(|e| {
                    !matches!(
                        e,
                        egui::Event::Key {
                            key: Key::Enter,
                            ..
                        }
                    )
                });
            }
        });

        // No timer repaint: the worker owns the poll timer and every GuiTx send wakes the UI.

        let pal = self.palette();

        self.handle_frameless_resize(ctx);
        self.render_custom_title_bar(ctx, &pal);
        // Dev Inspector Panel
        if self.dev_mode && self.dev_panel_open {
            egui::TopBottomPanel::bottom("dev_inspector")
                .resizable(true)
                .default_height(250.0)
                .show(ctx, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.strong("Trình kiểm tra Dev & Giao thức");
                        ui.separator();
                        ui.selectable_value(
                            &mut self.dev_tab,
                            DevTab::Logs,
                            "Lưu lượng mạng (Logs)",
                        );
                        ui.selectable_value(
                            &mut self.dev_tab,
                            DevTab::Scanner,
                            "Quét lỗi hệ thống (Scanner)",
                        );
                        ui.selectable_value(
                            &mut self.dev_tab,
                            DevTab::CryptoState,
                            "Trạng thái Crypto & Keys",
                        );
                        ui.separator();
                        if ui.button("Làm mới quét").clicked() {
                            self.scanner_findings = logger::scan_logs();
                        }
                        if ui.button("Xóa log").clicked() {
                            logger::clear_entries();
                            self.selected_log = None;
                            self.scanner_findings.clear();
                        }
                    });
                    ui.separator();

                    match self.dev_tab {
                        DevTab::Logs => self.render_dev_logs(ui),
                        DevTab::Scanner => self.render_dev_scanner(ui),
                        DevTab::CryptoState => self.render_dev_crypto(ui),
                    }
                });
        }

        // Main Workspace
        if !self.is_logged_in {
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(pal.bg_chat_canvas))
                .show(ctx, |ui| {
                    self.render_login_screen(ui, &pal);
                });
        } else {
            self.render_chat_workspace(ctx, &pal);
        }

        // Auto-notification modal for new GitHub Releases (works on both Login & Chat screens)
        self.render_update_notification_modal(ctx, &pal);

        if !self.is_window_maximized(ctx) {
            let screen = ctx.screen_rect();
            ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                Id::new("window_border_overlay"),
            ))
            .rect_stroke(
                screen.shrink(0.5),
                Rounding::ZERO,
                Stroke::new(1.0_f32, pal.border),
            );
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let _ = self.settings.save();
        std::process::exit(0);
    }
}

// ---------------------------------------------------------------------------
// Screen Renderers
// ---------------------------------------------------------------------------

impl ZaloGuiApp {
    fn resize_cursor_for(dir: egui::ResizeDirection) -> CursorIcon {
        match dir {
            egui::ResizeDirection::NorthWest | egui::ResizeDirection::SouthEast => {
                CursorIcon::ResizeNwSe
            }
            egui::ResizeDirection::NorthEast | egui::ResizeDirection::SouthWest => {
                CursorIcon::ResizeNeSw
            }
            egui::ResizeDirection::West | egui::ResizeDirection::East => {
                CursorIcon::ResizeHorizontal
            }
            egui::ResizeDirection::North | egui::ResizeDirection::South => {
                CursorIcon::ResizeVertical
            }
        }
    }

    #[cfg(target_os = "windows")]
    fn resolve_windows_hwnd(&mut self) -> usize {
        if !win32_frameless::is_valid_window(self.win_hwnd) {
            self.win_hwnd = win32_frameless::find_window_hwnd();
        }
        self.win_hwnd
    }

    fn ensure_windows_frameless_subclass(&mut self, ctx: &egui::Context) {
        #[cfg(target_os = "windows")]
        {
            let hwnd = self.resolve_windows_hwnd();
            if hwnd != 0 {
                let (newly_subclassed, style_changed) =
                    win32_frameless::install_frameless_subclass(hwnd);
                if newly_subclassed || style_changed {
                    ctx.request_repaint();
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        let _ = ctx;
    }

    fn is_window_maximized(&self, ctx: &egui::Context) -> bool {
        #[cfg(target_os = "windows")]
        if self.win_maximized {
            return true;
        }
        ctx.input(|i| i.viewport().maximized.unwrap_or(false))
    }

    fn toggle_maximize(&mut self, ctx: &egui::Context) {
        #[cfg(target_os = "windows")]
        {
            let hwnd = self.resolve_windows_hwnd();
            if hwnd != 0 {
                if !self.win_maximized {
                    self.win_restore_rect = win32_frameless::get_window_rect(hwnd);
                    if let Some(work) = win32_frameless::get_monitor_work_area(hwnd) {
                        win32_frameless::set_window_rect(hwnd, work, true);
                        self.win_maximized = true;
                        ctx.request_repaint();
                        return;
                    }
                } else {
                    let target = self.win_restore_rect.take().unwrap_or_else(|| {
                        let ppp = ctx.pixels_per_point().max(1.0);
                        let w = (1000.0 * ppp).round() as i32;
                        let h = (640.0 * ppp).round() as i32;
                        if let Some(work) = win32_frameless::get_monitor_work_area(hwnd) {
                            let left = work.left + (work.width() - w).max(0) / 2;
                            let top = work.top + (work.height() - h).max(0) / 2;
                            WinRect {
                                left,
                                top,
                                right: left + w,
                                bottom: top + h,
                            }
                        } else {
                            WinRect {
                                left: 80,
                                top: 80,
                                right: 80 + w,
                                bottom: 80 + h,
                            }
                        }
                    });
                    win32_frameless::set_window_rect(hwnd, target, true);
                    self.win_maximized = false;
                    ctx.request_repaint();
                    return;
                }
            }
        }
        let is_max = self.is_window_maximized(ctx);
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_max));
    }

    fn restore_before_drag_if_maximized(&mut self, ctx: &egui::Context) {
        #[cfg(target_os = "windows")]
        if self.win_maximized {
            let hwnd = self.resolve_windows_hwnd();
            if hwnd != 0 {
                let ppp = ctx.pixels_per_point().max(1.0);
                let fallback_w = (1000.0 * ppp).round() as i32;
                let fallback_h = (640.0 * ppp).round() as i32;
                let min_w = (480.0 * ppp).round() as i32;
                let min_h = (360.0 * ppp).round() as i32;
                let prev = self.win_restore_rect.take();
                let rw = prev.map_or(fallback_w, |r| r.width()).max(min_w);
                let rh = prev.map_or(fallback_h, |r| r.height()).max(min_h);
                if let (Some((cx, cy)), Some(work)) = (
                    win32_frameless::get_cursor_pos(),
                    win32_frameless::get_monitor_work_area(hwnd),
                ) {
                    let frac =
                        ((cx - work.left) as f32 / work.width().max(1) as f32).clamp(0.05, 0.95);
                    let left = cx - (rw as f32 * frac).round() as i32;
                    let top = (cy - (16.0 * ppp).round() as i32).max(work.top);
                    win32_frameless::set_window_rect(
                        hwnd,
                        WinRect {
                            left,
                            top,
                            right: left + rw,
                            bottom: top + rh,
                        },
                        true,
                    );
                }
                self.win_maximized = false;
            }
        }
        #[cfg(not(target_os = "windows"))]
        let _ = ctx;
    }

    fn handle_frameless_resize(&mut self, ctx: &egui::Context) {
        if self.is_window_maximized(ctx) {
            #[cfg(target_os = "windows")]
            {
                self.win_resize_drag = None;
            }
            return;
        }

        #[cfg(target_os = "windows")]
        if let Some(mut drag) = self.win_resize_drag {
            if ctx.input(|i| i.pointer.primary_down()) {
                ctx.set_cursor_icon(Self::resize_cursor_for(drag.dir));
                let hwnd = self.resolve_windows_hwnd();
                if hwnd != 0 {
                    if let Some((cx, cy)) = win32_frameless::get_cursor_pos() {
                        let ppp = ctx.pixels_per_point().max(1.0);
                        let min_w = (480.0 * ppp).round() as i32;
                        let min_h = (360.0 * ppp).round() as i32;
                        let target = compute_resized_rect(
                            drag.start_rect,
                            drag.dir,
                            cx.saturating_sub(drag.start_cursor_x),
                            cy.saturating_sub(drag.start_cursor_y),
                            min_w,
                            min_h,
                        );
                        if target != drag.last_rect {
                            win32_frameless::set_window_rect(hwnd, target, false);
                            drag.last_rect = target;
                            self.win_resize_drag = Some(drag);
                        }
                    }
                }
                ctx.request_repaint();
                return;
            }
            self.win_resize_drag = None;
        }

        let screen = ctx.screen_rect();
        let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
            return;
        };
        // Keep the top-right window control buttons (Close / Max / Min / Theme) clickable
        // even right up to the top-right corner of the frameless window.
        if pos.y < screen.top() + 34.0 && pos.x >= screen.right() - 180.0 {
            return;
        }
        let border = 5.0_f32;
        let corner = 9.0_f32;
        let left = (pos.x - screen.left()).abs() <= border;
        let right = (screen.right() - pos.x).abs() <= border;
        let top = (pos.y - screen.top()).abs() <= border;
        let bottom = (screen.bottom() - pos.y).abs() <= border;

        let left_c = (pos.x - screen.left()).abs() <= corner;
        let right_c = (screen.right() - pos.x).abs() <= corner;
        let top_c = (pos.y - screen.top()).abs() <= corner;
        let bottom_c = (screen.bottom() - pos.y).abs() <= corner;

        let dir = if top_c && left_c && (top || left) {
            Some(egui::ResizeDirection::NorthWest)
        } else if top_c && right_c && (top || right) {
            Some(egui::ResizeDirection::NorthEast)
        } else if bottom_c && left_c && (bottom || left) {
            Some(egui::ResizeDirection::SouthWest)
        } else if bottom_c && right_c && (bottom || right) {
            Some(egui::ResizeDirection::SouthEast)
        } else if left {
            Some(egui::ResizeDirection::West)
        } else if right {
            Some(egui::ResizeDirection::East)
        } else if top {
            Some(egui::ResizeDirection::North)
        } else if bottom {
            Some(egui::ResizeDirection::South)
        } else {
            None
        };

        if let Some(dir) = dir {
            ctx.set_cursor_icon(Self::resize_cursor_for(dir));
            if ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary)) {
                #[cfg(target_os = "windows")]
                {
                    let hwnd = self.resolve_windows_hwnd();
                    if let (Some((cx, cy)), Some(rect)) = (
                        win32_frameless::get_cursor_pos(),
                        win32_frameless::get_window_rect(hwnd),
                    ) {
                        self.win_resize_drag = Some(WinResizeDrag {
                            dir,
                            start_cursor_x: cx,
                            start_cursor_y: cy,
                            start_rect: rect,
                            last_rect: rect,
                        });
                        ctx.request_repaint();
                        return;
                    }
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
            }
        }
    }

    fn render_custom_title_bar(&mut self, ctx: &egui::Context, pal: &ThemePalette) {
        let is_maximized = self.is_window_maximized(ctx);
        let mut toggle_max_requested = false;
        let mut drag_started = false;
        #[cfg(target_os = "windows")]
        let resizing_now = self.win_resize_drag.is_some();
        #[cfg(not(target_os = "windows"))]
        let resizing_now = false;
        let total_unread: usize = self.unread_counts.values().sum();
        if self.last_synced_icon_unread != Some(total_unread) {
            self.last_synced_icon_unread = Some(total_unread);
            if let Some(icon) = crate::gui::theme::load_app_icon_with_badge(total_unread) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(std::sync::Arc::new(icon))));
            }
        }

        egui::TopBottomPanel::top("custom_title_bar")
            .exact_height(32.0)
            .frame(
                egui::Frame::none()
                    .fill(pal.bg_app)
                    .inner_margin(Margin {
                        left: 12.0,
                        right: 0.0,
                        top: 0.0,
                        bottom: 0.0,
                    })
                    .stroke(Stroke::new(1.0_f32, pal.border)),
            )
            .show(ctx, |ui| {
                let bar_rect = ui.max_rect();
                let mut buttons_left_x = bar_rect.right();

                ui.horizontal_centered(|ui| {
                    // 1. App Logo (20x20)
                    let (logo_rect, _) =
                        ui.allocate_exact_size(Vec2::new(20.0, 20.0), Sense::hover());
                    if let Some(tex) = &self.app_logo_texture {
                        ui.painter().image(
                            tex.id(),
                            logo_rect,
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    } else {
                        ui.painter()
                            .rect_filled(logo_rect, Rounding::same(5.0), pal.primary);
                    }

                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("ctdozlite")
                            .color(pal.text_primary)
                            .size(13.5)
                            .strong(),
                    );

                    let compact_title_bar = bar_rect.width() < 560.0;
                    // Subtle "Native v<version>" pill badge (hidden below 560px to preserve controls)
                    if !compact_title_bar {
                        egui::Frame::none()
                            .fill(pal.primary.linear_multiply(0.14))
                            .rounding(Rounding::same(4.0))
                            .inner_margin(Margin::symmetric(5.0, 1.5))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!("Native v{}", env!("CARGO_PKG_VERSION")))
                                        .color(pal.primary)
                                        .size(10.5)
                                        .strong(),
                                );
                            });
                    }

                    if total_unread > 0 {
                        ui.add_space(4.0);
                        let unread_lbl = if bar_rect.width() < 440.0 {
                            total_unread.to_string()
                        } else {
                            format!("{total_unread} tin mới")
                        };
                        egui::Frame::none()
                            .fill(pal.danger)
                            .rounding(Rounding::same(8.0))
                            .inner_margin(Margin::symmetric(6.0, 1.5))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(unread_lbl)
                                        .color(Color32::WHITE)
                                        .size(10.5)
                                        .strong(),
                                );
                            });
                    } else if !compact_title_bar && self.is_logged_in && !self.user_name.is_empty()
                    {
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(format!("·  {}", truncate_chars(&self.user_name, 18)))
                                .color(pal.text_muted)
                                .size(12.0),
                        );
                    }

                    // 2. Right-side Window Control Buttons
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;

                        // Close Button (46x32)
                        let (close_rect, close_resp) =
                            ui.allocate_exact_size(Vec2::new(46.0, 32.0), Sense::click());
                        if close_resp.hovered() {
                            ui.painter().rect_filled(
                                close_rect,
                                Rounding::ZERO,
                                Color32::from_rgb(232, 17, 35),
                            );
                        }
                        let close_fg = if close_resp.hovered() {
                            Color32::WHITE
                        } else {
                            pal.text_secondary
                        };
                        let cc = close_rect.center();
                        let d = 4.6_f32;
                        ui.painter().line_segment(
                            [Pos2::new(cc.x - d, cc.y - d), Pos2::new(cc.x + d, cc.y + d)],
                            Stroke::new(1.4_f32, close_fg),
                        );
                        ui.painter().line_segment(
                            [Pos2::new(cc.x + d, cc.y - d), Pos2::new(cc.x - d, cc.y + d)],
                            Stroke::new(1.4_f32, close_fg),
                        );
                        let close_resp = close_resp.on_hover_text("Đóng ứng dụng (Ctrl+Q)");
                        if close_resp.clicked() {
                            self.allow_quit = true;
                            if let Some(tx) = WORKER_TX.get() {
                                let _ = tx.send(WorkerCmd::EndCall);
                            }
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }

                        // Maximize / Restore Button (46x32)
                        let (max_rect, max_resp) =
                            ui.allocate_exact_size(Vec2::new(46.0, 32.0), Sense::click());
                        if max_resp.hovered() {
                            ui.painter()
                                .rect_filled(max_rect, Rounding::ZERO, pal.bg_hover);
                        }
                        let max_fg = if max_resp.hovered() {
                            pal.text_primary
                        } else {
                            pal.text_secondary
                        };
                        let mc = max_rect.center();
                        if is_maximized {
                            let back = Rect::from_center_size(
                                Pos2::new(mc.x + 1.2, mc.y - 1.2),
                                Vec2::splat(7.8),
                            );
                            let front = Rect::from_center_size(
                                Pos2::new(mc.x - 1.2, mc.y + 1.2),
                                Vec2::splat(7.8),
                            );
                            ui.painter()
                                .rect_stroke(back, Rounding::same(1.0), Stroke::new(1.25_f32, max_fg));
                            ui.painter().rect_filled(
                                front,
                                Rounding::same(1.0),
                                if max_resp.hovered() {
                                    pal.bg_hover
                                } else {
                                    pal.bg_app
                                },
                            );
                            ui.painter()
                                .rect_stroke(front, Rounding::same(1.0), Stroke::new(1.25_f32, max_fg));
                        } else {
                            let sq = Rect::from_center_size(mc, Vec2::splat(9.5));
                            ui.painter()
                                .rect_stroke(sq, Rounding::same(1.2), Stroke::new(1.3_f32, max_fg));
                        }
                        if max_resp
                            .on_hover_text(if is_maximized {
                                "Khôi phục kích thước"
                            } else {
                                "Phóng to toàn màn hình"
                            })
                            .clicked()
                        {
                            toggle_max_requested = true;
                        }

                        // Minimize Button (46x32)
                        let (min_rect, min_resp) =
                            ui.allocate_exact_size(Vec2::new(46.0, 32.0), Sense::click());
                        if min_resp.hovered() {
                            ui.painter()
                                .rect_filled(min_rect, Rounding::ZERO, pal.bg_hover);
                        }
                        let min_fg = if min_resp.hovered() {
                            pal.text_primary
                        } else {
                            pal.text_secondary
                        };
                        let mnc = min_rect.center();
                        ui.painter().line_segment(
                            [Pos2::new(mnc.x - 5.0, mnc.y), Pos2::new(mnc.x + 5.0, mnc.y)],
                            Stroke::new(1.4_f32, min_fg),
                        );
                        if min_resp.on_hover_text("Thu nhỏ").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }

                        // Quick Theme Toggle (36x32)
                        let (th_rect, th_resp) =
                            ui.allocate_exact_size(Vec2::new(36.0, 32.0), Sense::click());
                        if th_resp.hovered() {
                            ui.painter()
                                .rect_filled(th_rect, Rounding::ZERO, pal.bg_hover);
                        }
                        let th_icon = if self.dark_mode {
                            lucide::SUN
                        } else {
                            lucide::MOON
                        };
                        ui.painter().text(
                            th_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            th_icon,
                            FontId::proportional(13.5),
                            if th_resp.hovered() {
                                pal.text_primary
                            } else {
                                pal.text_muted
                            },
                        );
                        if th_resp
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .on_hover_text("Chuyển đổi giao diện Sáng / Tối")
                            .clicked()
                        {
                            self.dark_mode = !self.dark_mode;
                            self.settings.theme_mode = if self.dark_mode {
                                "dark".to_string()
                            } else {
                                "light".to_string()
                            };
                            let _ = self.settings.save();
                            self.theme_dirty = true;
                        }

                        buttons_left_x = th_rect.left();

                        if let Some(ref upd) = self.update_info {
                            if upd.has_update && !compact_title_bar {
                                ui.add_space(6.0);
                                let badge_txt = if self.update_downloading {
                                    let (dl, tot) = self.update_progress.unwrap_or((0, 0));
                                    if tot > 0 {
                                        let pct = ((dl as f64 / tot as f64) * 100.0).clamp(0.0, 100.0);
                                        format!("{} Đang cập nhật {pct:.0}%", lucide::REFRESH)
                                    } else {
                                        format!("{} Đang tải cập nhật…", lucide::REFRESH)
                                    }
                                } else {
                                    format!("{} Cập nhật mới v{}", lucide::SPARKLES, upd.latest_version)
                                };
                                let upd_pill = egui::Button::new(
                                    RichText::new(badge_txt)
                                        .size(11.2)
                                        .color(pal.primary_fg)
                                        .strong(),
                                )
                                .fill(pal.primary)
                                .rounding(Rounding::same(5.0));
                                let r = ui
                                    .add(upd_pill)
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .on_hover_text(format!(
                                        "Đã có phiên bản v{} trên GitHub Releases — Bấm để xem chi tiết & cập nhật tự động",
                                        upd.latest_version
                                    ));
                                buttons_left_x = r.rect.left() - 8.0;
                                if r.clicked() {
                                    self.update_modal_open = true;
                                    self.update_banner_dismissed = false;
                                }
                            }
                        }

                        if !self.is_logged_in && !self.threads.is_empty() {
                            ui.add_space(8.0);
                            let hist_btn = egui::Button::new(
                                RichText::new("Xem lịch sử đã lưu")
                                    .size(11.5)
                                    .color(pal.primary)
                                    .strong(),
                            )
                            .fill(pal.primary.linear_multiply(0.12))
                            .stroke(Stroke::new(1.0_f32, pal.primary))
                            .rounding(Rounding::same(5.0));
                            let r = ui
                                .add(hist_btn)
                                .on_hover_cursor(CursorIcon::PointingHand);
                            buttons_left_x = r.rect.left() - 8.0;
                            if r.clicked() {
                                self.is_logged_in = true;
                            }
                        }
                    });
                });

                // 3. Interactive drag / double-click region covering everything left of the control buttons
                let drag_rect = Rect::from_min_max(
                    bar_rect.min,
                    Pos2::new((buttons_left_x - 2.0).max(bar_rect.left() + 40.0), bar_rect.max.y),
                );
                let drag_resp = ui.interact(
                    drag_rect,
                    Id::new("custom_title_bar_drag"),
                    Sense::click_and_drag(),
                );
                if !resizing_now && drag_resp.double_clicked() {
                    toggle_max_requested = true;
                } else if !resizing_now && drag_resp.drag_started_by(egui::PointerButton::Primary) {
                    drag_started = true;
                }
            });

        if toggle_max_requested {
            self.toggle_maximize(ctx);
        } else if drag_started {
            self.restore_before_drag_if_maximized(ctx);
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
    }

    fn render_login_screen(&mut self, ui: &mut egui::Ui, pal: &ThemePalette) {
        egui::ScrollArea::vertical()
            .id_salt("login_screen_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    let avail_h = ui.available_height();
                    ui.add_space((avail_h * 0.05).max(16.0));
                    if let Some(tex) = &self.app_logo_texture {
                        ui.image((tex.id(), Vec2::new(64.0, 64.0)));
                        ui.add_space(10.0);
                    }
                    ui.label(
                        RichText::new("Đăng nhập Zalo")
                            .font(FontId::new(24.0, crate::gui::theme::font_family_semibold()))
                            .color(pal.primary),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Khởi chạy nhẹ, bảo mật và đồng bộ tin nhắn thời gian thực")
                            .color(pal.text_secondary)
                            .size(13.5),
                    );
                    ui.add_space(18.0);

                    let card_w = (ui.available_width() - 32.0).clamp(240.0, 360.0);
                    let qr_box_side = (card_w - 8.0).min(268.0);
                    let qr_img_side = (qr_box_side - 28.0).max(160.0);
                    let pad_x = if card_w < 300.0 { 16.0 } else { 28.0 };

                    egui::Frame::none()
                        .fill(pal.bg_elevated)
                        .rounding(Rounding::same(16.0))
                        .stroke(Stroke::new(1.0_f32, pal.border))
                        .inner_margin(Margin::symmetric(pad_x, 24.0))
                        .show(ui, |ui| {
                            ui.set_width(card_w);
                            ui.vertical_centered(|ui| {
                                // Segmented Tab Switcher with smooth animated sliding pill
                                egui::Frame::none()
                                    .fill(pal.bg_input_track)
                                    .rounding(Rounding::same(11.0))
                                    .inner_margin(Margin::same(4.0))
                                    .show(ui, |ui| {
                                        let slide_t = self.anim_bool(
                                            ui.ctx(),
                                            Id::new("login_tab_slide"),
                                            self.login_tab == LoginTab::Password,
                                            0.16,
                                        );
                                        let track_w = ui.available_width();
                                        let tab_w = ((track_w - 4.0) / 2.0).max(80.0);
                                        let (track_rect, _) = ui.allocate_exact_size(
                                            Vec2::new(track_w, 34.0),
                                            Sense::hover(),
                                        );
                                        let left_rect = Rect::from_min_size(
                                            track_rect.min,
                                            Vec2::new(tab_w, 34.0),
                                        );
                                        let right_rect = Rect::from_min_size(
                                            Pos2::new(track_rect.right() - tab_w, track_rect.top()),
                                            Vec2::new(tab_w, 34.0),
                                        );
                                        let pill_x = egui::lerp(left_rect.left()..=right_rect.left(), slide_t);
                                        let pill_rect = Rect::from_min_size(
                                            Pos2::new(pill_x, track_rect.top()),
                                            Vec2::new(tab_w, 34.0),
                                        );
                                        ui.painter().rect_filled(
                                            pill_rect,
                                            Rounding::same(8.0),
                                            pal.primary,
                                        );

                                        for (rect, tab, title) in [
                                            (left_rect, LoginTab::QrCode, "Quét mã QR"),
                                            (right_rect, LoginTab::Password, "Số điện thoại"),
                                        ] {
                                            let resp = ui.interact(
                                                rect,
                                                Id::new(("login_tab_btn", title)),
                                                Sense::click(),
                                            );
                                            let active = self.login_tab == tab;
                                            let label_col = if active {
                                                pal.primary_fg
                                            } else if resp.hovered() {
                                                pal.text_primary
                                            } else {
                                                pal.text_secondary
                                            };
                                            ui.painter().text(
                                                rect.center(),
                                                egui::Align2::CENTER_CENTER,
                                                title,
                                                FontId::new(13.0, crate::gui::theme::font_family_semibold()),
                                                label_col,
                                            );
                                            if resp
                                                .on_hover_cursor(CursorIcon::PointingHand)
                                                .clicked()
                                            {
                                                self.login_tab = tab;
                                            }
                                        }
                                    });

                                ui.add_space(18.0);

                                match self.login_tab {
                                    LoginTab::QrCode => {
                                        if let Some(tex) = &self.qr_texture {
                                            egui::Frame::none()
                                                .fill(Color32::WHITE)
                                                .rounding(Rounding::same(12.0))
                                                .stroke(Stroke::new(1.0_f32, pal.border))
                                                .inner_margin(Margin::same(14.0))
                                                .show(ui, |ui| {
                                                    ui.image((tex.id(), Vec2::splat(qr_img_side)));
                                                });
                                        } else {
                                            let (rect, _) = ui.allocate_exact_size(
                                                Vec2::splat(qr_box_side),
                                                Sense::hover(),
                                            );
                                            ui.painter().rect_filled(
                                                rect,
                                                Rounding::same(12.0),
                                                pal.bg_input,
                                            );
                                            ui.painter().rect_stroke(
                                                rect,
                                                Rounding::same(12.0),
                                                Stroke::new(1.0_f32, pal.border),
                                            );
                                            ui.painter().text(
                                                rect.center(),
                                                egui::Align2::CENTER_CENTER,
                                                "Đang tải mã QR…",
                                                FontId::proportional(14.5),
                                                pal.text_secondary,
                                            );
                                        }

                                        ui.add_space(14.0);
                                        ui.label(
                                            RichText::new(&self.qr_status)
                                                .color(pal.text_primary)
                                                .size(13.5),
                                        );
                                        ui.add_space(12.0);

                                        let refresh_btn = egui::Button::new(
                                            RichText::new("Tạo / Làm mới mã QR")
                                                .size(13.5)
                                                .color(pal.primary_fg)
                                                .strong(),
                                        )
                                        .fill(pal.primary)
                                        .rounding(Rounding::same(8.0))
                                        .min_size(Vec2::new(card_w.min(220.0), 36.0));

                                        if ui
                                            .add(refresh_btn)
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            self.login_in_progress = true;
                                            self.qr_status = "Đang xin mã QR…".into();
                                            if let Some(tx) = WORKER_TX.get() {
                                                let _ = tx.send(WorkerCmd::RequestQr);
                                            }
                                        }
                                    }
                                    LoginTab::Password => {
                                        let phone_id = Id::new("login_phone_input");
                                        let pass_id = Id::new("login_password_input");
                                        let phone_focused = ui.memory(|m| m.has_focus(phone_id));
                                        let pass_focused = ui.memory(|m| m.has_focus(pass_id));

                                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                            ui.label(
                                                RichText::new("Số điện thoại")
                                                    .font(FontId::new(13.0, crate::gui::theme::font_family_semibold()))
                                                    .color(pal.text_secondary),
                                            );
                                            ui.add_space(4.0);
                                            crate::gui::theme::styled_input_frame(pal, phone_focused).show(ui, |ui| {
                                                ui.add_sized(
                                                    [ui.available_width(), 22.0],
                                                    egui::TextEdit::singleline(&mut self.phone_input)
                                                        .id(phone_id)
                                                        .frame(false)
                                                        .hint_text("Ví dụ: 0912345678"),
                                                );
                                            });
                                            ui.add_space(12.0);

                                            ui.label(
                                                RichText::new("Mật khẩu")
                                                    .font(FontId::new(13.0, crate::gui::theme::font_family_semibold()))
                                                    .color(pal.text_secondary),
                                            );
                                            ui.add_space(4.0);
                                            crate::gui::theme::styled_input_frame(pal, pass_focused).show(ui, |ui| {
                                                ui.add_sized(
                                                    [ui.available_width(), 22.0],
                                                    egui::TextEdit::singleline(&mut self.password_input)
                                                        .id(pass_id)
                                                        .frame(false)
                                                        .password(true)
                                                        .hint_text("Nhập mật khẩu Zalo"),
                                                );
                                            });
                                            ui.add_space(16.0);

                                            if !self.login_status_msg.is_empty() {
                                                ui.label(
                                                    RichText::new(&self.login_status_msg)
                                                        .color(pal.danger_text)
                                                        .size(12.5),
                                                );
                                                ui.add_space(8.0);
                                            }
                                        });

                                        // Render primary submit button in centered layout so its label never shifts left.
                                        let btn = egui::Button::new(
                                            RichText::new("Đăng nhập")
                                                .color(pal.primary_fg)
                                                .size(14.0)
                                                .strong(),
                                        )
                                        .fill(pal.primary)
                                        .rounding(Rounding::same(8.0))
                                        .min_size(Vec2::new(ui.available_width(), 38.0));

                                        if ui
                                            .add_enabled(!self.login_in_progress, btn)
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            self.login_in_progress = true;
                                            self.login_status_msg = "Đang kết nối…".into();
                                            if let Some(tx) = WORKER_TX.get() {
                                                let _ = tx.send(WorkerCmd::LoginPassword {
                                                    phone: self.phone_input.clone(),
                                                    pass: self.password_input.clone(),
                                                });
                                            }
                                        }
                                    }
                                }
                            });
                        });

                    // Nút quay lại khi đang thêm tài khoản mới mà đổi ý (#103).
                    if let Some(prev_uin) = self.pending_add_account_uin.clone() {
                        ui.add_space(12.0);
                        let back = egui::Button::new(
                            RichText::new("← Hủy, quay lại tài khoản trước")
                                .size(13.0)
                                .color(pal.primary),
                        )
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::new(1.0_f32, pal.primary.linear_multiply(0.5)))
                        .rounding(Rounding::same(8.0))
                        .min_size(Vec2::new(card_w.min(280.0), 34.0));
                        if ui
                            .add(back)
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .clicked()
                        {
                            self.pending_add_account_uin = None;
                            self.status_msg = "Đã hủy thêm tài khoản".into();
                            if let Some(tx) = WORKER_TX.get() {
                                let _ = tx.send(WorkerCmd::SwitchAccount {
                                    target_uin: prev_uin,
                                });
                            }
                        }
                    }

                    // Danh sách tài khoản đã lưu trên thiết bị để đăng nhập lại 1-click (#80, #130).
                    self.render_saved_accounts_on_login(ui, pal);
                    ui.add_space(20.0);
                });
            });
    }

    fn show_toast(&mut self, msg: impl Into<String>) {
        let text = msg.into();
        self.status_msg = text.clone();
        self.toast_notice = Some((text, std::time::Instant::now()));
    }
}

fn settings_default_animation_speed() -> f32 {
    1.0
}

pub(super) fn resolve_dark_mode(theme_mode: &str, ctx: &egui::Context) -> bool {
    match theme_mode {
        "light" => false,
        "system" => {
            if let Some(sys) = ctx.input(|i| i.raw.system_theme) {
                sys == egui::Theme::Dark
            } else {
                #[cfg(target_os = "windows")]
                {
                    detect_windows_os_dark_mode().unwrap_or(true)
                }
                #[cfg(not(target_os = "windows"))]
                {
                    true
                }
            }
        }
        _ => true,
    }
}

#[cfg(target_os = "windows")]
fn detect_windows_os_dark_mode() -> Option<bool> {
    // Query HKCU\...\Personalize\AppsUseLightTheme without extra crate deps (0 = dark, 1 = light).
    type Hkey = isize;
    const HKEY_CURRENT_USER: Hkey = -0x7FFF_FFFF_isize; // 0x80000001
    const RRF_RT_REG_DWORD: u32 = 0x0000_0010;
    #[link(name = "advapi32")]
    extern "system" {
        fn RegGetValueW(
            hkey: Hkey,
            lpSubKey: *const u16,
            lpValue: *const u16,
            dwFlags: u32,
            pdwType: *mut u32,
            pvData: *mut core::ffi::c_void,
            pcbData: *mut u32,
        ) -> i32;
    }
    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    let subkey = to_wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let val_name = to_wide("AppsUseLightTheme");
    let mut val: u32 = 1;
    let mut size: u32 = std::mem::size_of::<u32>() as u32;
    // SAFETY: Stack pointers (`val`, `size`) and null-terminated UTF-16 buffers stay valid across synchronous Win32 API call.
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            val_name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut val as *mut u32).cast(),
            &mut size,
        )
    };
    if rc == 0 {
        Some(val == 0)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{is_newer_semver, phone_sync_completion_message};

    #[test]
    fn sent_message_reconciles_when_server_push_arrives_first() {
        let mut thread = crate::chat::Thread::default();
        let server = crate::chat::ChatMessage {
            id: "server-a".into(),
            client_message_id: "client-a".into(),
            text: "Server push".into(),
            is_outgoing: true,
            delivery: crate::message_model::DeliveryState::Delivered,
            ..Default::default()
        };
        super::merge_sent_message(&mut thread, server.clone());
        for local_id in ["out_client-a", "server-a"] {
            super::merge_sent_message(
                &mut thread,
                crate::chat::ChatMessage {
                    id: local_id.into(),
                    client_message_id: "client-a".into(),
                    text: "Local echo".into(),
                    is_outgoing: true,
                    ..Default::default()
                },
            );
            assert_eq!(thread.messages.len(), 1);
            assert_eq!(thread.messages[0].id, "server-a");
            assert_eq!(thread.messages[0].delivery, server.delivery);
        }
    }

    #[test]
    fn delayed_local_ack_preserves_server_chronology() {
        let mut thread = crate::chat::Thread::default();
        for (id, client, ts) in [
            ("server-a", "client-a", 1000),
            ("server-b", "client-b", 2000),
        ] {
            thread.messages.push(crate::chat::ChatMessage {
                id: id.into(),
                client_message_id: client.into(),
                ts,
                time: ts.to_string(),
                is_outgoing: true,
                ..Default::default()
            });
        }
        for ack_id in ["out_client-a", "server-a"] {
            super::merge_sent_message(
                &mut thread,
                crate::chat::ChatMessage {
                    id: ack_id.into(),
                    client_message_id: "client-a".into(),
                    ts: 3000,
                    time: "3000".into(),
                    is_outgoing: true,
                    ..Default::default()
                },
            );
            let chronology: Vec<_> = thread
                .messages
                .iter()
                .map(|message| (message.id.as_str(), message.ts, message.time.as_str()))
                .collect();
            assert_eq!(
                chronology,
                [("server-a", 1000, "1000"), ("server-b", 2000, "2000")]
            );
        }
    }

    #[test]
    fn semver_comparison_handles_v_prefix_and_prerelease() {
        assert!(is_newer_semver("v0.0.2", "0.0.1"));
        assert!(is_newer_semver("0.1.0", "0.0.9"));
        assert!(is_newer_semver("V1.0.0-beta.1", "0.9.9"));
        assert!(!is_newer_semver("v0.0.1", "0.0.1"));
        assert!(!is_newer_semver("0.0.1", "0.0.2"));
    }

    #[test]
    fn phone_sync_completion_does_not_claim_phone_messages_were_imported() {
        let none = phone_sync_completion_message(0, 0, false, None);
        assert!(none.contains("Lịch sử điện thoại chưa được tải vào ứng dụng"));
        assert!(none.contains("0 tin mới từ Cloud/Nhóm"));

        let cloud = phone_sync_completion_message(12, 3, false, None);
        assert!(cloud.contains("12 tin mới từ Cloud/Nhóm (3 hội thoại)"));
        assert!(cloud.contains("Lịch sử điện thoại chưa được tải vào ứng dụng"));

        let failed = phone_sync_completion_message(0, 0, false, Some("network"));
        assert!(failed.contains("Đồng bộ thất bại: network"));
    }

    #[test]
    fn test_app_settings_font_and_color_defaults_and_serde() {
        let default_settings = super::AppSettings::default();
        assert_eq!(default_settings.font_family, "default");
        assert_eq!(default_settings.font_scale, 1.0);
        assert!(default_settings.animations_enabled);
        assert_eq!(default_settings.animation_speed, 1.0);
        assert_eq!(default_settings.custom_primary_color, None);
        assert_eq!(default_settings.custom_accent_color, None);
        assert_eq!(default_settings.custom_bubble_out_bg, None);
        assert_eq!(default_settings.custom_bubble_in_bg, None);

        let mut custom = default_settings.clone();
        custom.font_family = "segoe_ui".into();
        custom.font_scale = 1.15;
        custom.animations_enabled = false;
        custom.animation_speed = 1.5;
        custom.custom_primary_color = Some("#0068FF".into());
        custom.custom_accent_color = Some("#059669".into());
        custom.custom_bubble_out_bg = Some("#25374D".into());
        custom.custom_bubble_in_bg = Some("#323842".into());

        let json = serde_json::to_string(&custom).expect("serialize settings");
        let loaded: super::AppSettings = serde_json::from_str(&json).expect("deserialize settings");
        assert_eq!(loaded.font_family, "segoe_ui");
        assert_eq!(loaded.font_scale, 1.15);
        assert!(!loaded.animations_enabled);
        assert_eq!(loaded.animation_speed, 1.5);
        assert_eq!(loaded.custom_primary_color.as_deref(), Some("#0068FF"));
        assert_eq!(loaded.custom_accent_color.as_deref(), Some("#059669"));
        assert_eq!(loaded.custom_bubble_out_bg.as_deref(), Some("#25374D"));
        assert_eq!(loaded.custom_bubble_in_bg.as_deref(), Some("#323842"));
    }

    #[test]
    fn compute_resized_rect_handles_edges_and_clamps_minimums() {
        let base = super::WinRect {
            left: 100,
            top: 100,
            right: 1100,
            bottom: 800,
        };
        assert_eq!(base.width(), 1000);
        assert_eq!(base.height(), 700);

        let expanded = super::compute_resized_rect(
            base,
            eframe::egui::ResizeDirection::NorthWest,
            -40,
            -20,
            480,
            360,
        );
        assert_eq!(
            expanded,
            super::WinRect {
                left: 60,
                top: 80,
                right: 1100,
                bottom: 800,
            }
        );

        let clamped_nw = super::compute_resized_rect(
            base,
            eframe::egui::ResizeDirection::NorthWest,
            900,
            600,
            480,
            360,
        );
        assert_eq!(clamped_nw.width(), 480);
        assert_eq!(clamped_nw.height(), 360);
        assert_eq!(clamped_nw.right, 1100);
        assert_eq!(clamped_nw.bottom, 800);

        let clamped_se = super::compute_resized_rect(
            base,
            eframe::egui::ResizeDirection::SouthEast,
            -900,
            -600,
            480,
            360,
        );
        assert_eq!(clamped_se.width(), 480);
        assert_eq!(clamped_se.height(), 360);
        assert_eq!(clamped_se.left, 100);
        assert_eq!(clamped_se.top, 100);
    }

    #[test]
    fn sticker_fetch_retry_respects_cooldown_and_max_attempts() {
        let mut failures = std::collections::HashMap::new();
        let t0 = std::time::Instant::now();
        assert!(super::can_attempt_sticker_fetch(&failures, "stk_1", t0));

        // 1st failure -> blocked during 30s cooldown, allowed after 30s.
        super::record_sticker_fetch_failure(&mut failures, "stk_1", t0);
        assert!(!super::can_attempt_sticker_fetch(
            &failures,
            "stk_1",
            t0 + std::time::Duration::from_secs(10)
        ));
        let t1 = t0 + std::time::Duration::from_secs(30);
        assert!(super::can_attempt_sticker_fetch(&failures, "stk_1", t1));

        // 2nd failure -> allowed once more after cooldown.
        super::record_sticker_fetch_failure(&mut failures, "stk_1", t1);
        let t2 = t1 + std::time::Duration::from_secs(30);
        assert!(super::can_attempt_sticker_fetch(&failures, "stk_1", t2));

        // 3rd failure reaches MAX_STICKER_FETCH_ATTEMPTS -> permanently stops auto-retry.
        super::record_sticker_fetch_failure(&mut failures, "stk_1", t2);
        assert!(!super::can_attempt_sticker_fetch(
            &failures,
            "stk_1",
            t2 + std::time::Duration::from_secs(3600)
        ));
    }

    #[test]
    fn truncate_chars_vietnamese_utf8_safety() {
        let s = "Nguyễn Thị Hồng Nhung — Tiếng Việt có dấu";
        let cut = super::truncate_chars(s, 16);
        assert_eq!(cut.chars().count(), 17);
        assert!(cut.ends_with('…'));
        assert!(cut.starts_with("Nguyễn Thị Hồng"));
        assert_eq!(super::truncate_chars("Ngắn", 16).as_ref(), "Ngắn");
    }

    #[test]
    fn app_settings_animations_serde_defaults_and_clamping() {
        let legacy: super::AppSettings = serde_json::from_str("{}").unwrap();
        assert!(legacy.animations_enabled);
        assert!((legacy.animation_speed - 1.0).abs() < f32::EPSILON);
        assert!((legacy.normalized_animation_speed() - 1.0).abs() < f32::EPSILON);

        let mut custom = super::AppSettings {
            animations_enabled: false,
            animation_speed: 3.5,
            ..Default::default()
        };
        assert!((custom.normalized_animation_speed() - 2.0).abs() < f32::EPSILON);
        custom.animation_speed = 0.1;
        assert!((custom.normalized_animation_speed() - 0.5).abs() < f32::EPSILON);

        let roundtrip: super::AppSettings =
            serde_json::from_str(&serde_json::to_string(&custom).unwrap()).unwrap();
        assert!(!roundtrip.animations_enabled);
        assert!((roundtrip.animation_speed - 0.1).abs() < 1e-4);
    }
}
