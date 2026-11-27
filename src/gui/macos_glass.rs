//! Native macOS behind-window frosted glass (`NSVisualEffectView`) bridge.

use std::ffi::{c_char, c_void, CStr};
use std::sync::atomic::{AtomicUsize, Ordering};

#[repr(C)]
#[derive(Copy, Clone)]
struct NSPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct NSSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct NSRect {
    origin: NSPoint,
    size: NSSize,
}

#[link(name = "AppKit", kind = "framework")]
extern "C" {}

#[link(name = "objc")]
extern "C" {
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> *mut c_void;
    fn objc_msgSend();
    #[cfg(target_arch = "x86_64")]
    fn objc_msgSend_stret();
}

extern "C" {
    fn ctdozlite_macos_install_reopen_handler();
    fn ctdozlite_macos_set_appearance(win_ptr: *mut c_void, view_ptr: *mut c_void, dark_mode: i32);
}

static VIBRANCY_WINDOW: AtomicUsize = AtomicUsize::new(0);
static VIBRANCY_VIEW: AtomicUsize = AtomicUsize::new(0);

const NS_VISUAL_EFFECT_MATERIAL_SIDEBAR: isize = 7;
const NS_VISUAL_EFFECT_MATERIAL_HUD_WINDOW: isize = 13;

fn material_for_theme(dark_mode: bool) -> isize {
    if dark_mode {
        NS_VISUAL_EFFECT_MATERIAL_HUD_WINDOW
    } else {
        NS_VISUAL_EFFECT_MATERIAL_SIDEBAR
    }
}

fn is_main_window_title(title: &[u8]) -> bool {
    title == b"ctdozlite"
        || (title.starts_with(b"(") && title.ends_with(b") ctdozlite"))
}

unsafe fn sel(name: &[u8]) -> *mut c_void {
    sel_registerName(name.as_ptr().cast())
}

unsafe fn cls(name: &[u8]) -> *mut c_void {
    objc_getClass(name.as_ptr().cast())
}

unsafe fn msg0_ptr(obj: *mut c_void, selector: *mut c_void) -> *mut c_void {
    let f: extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
        std::mem::transmute(objc_msgSend as *const ());
    f(obj, selector)
}

unsafe fn msg0_void(obj: *mut c_void, selector: *mut c_void) {
    let f: extern "C" fn(*mut c_void, *mut c_void) =
        std::mem::transmute(objc_msgSend as *const ());
    f(obj, selector);
}

unsafe fn msg0_usize(obj: *mut c_void, selector: *mut c_void) -> usize {
    let f: extern "C" fn(*mut c_void, *mut c_void) -> usize =
        std::mem::transmute(objc_msgSend as *const ());
    f(obj, selector)
}

unsafe fn msg0_rect(obj: *mut c_void, selector: *mut c_void) -> NSRect {
    #[cfg(target_arch = "aarch64")]
    {
        let f: extern "C" fn(*mut c_void, *mut c_void) -> NSRect =
            std::mem::transmute(objc_msgSend as *const ());
        f(obj, selector)
    }
    #[cfg(target_arch = "x86_64")]
    {
        let f: extern "C" fn(*mut NSRect, *mut c_void, *mut c_void) =
            std::mem::transmute(objc_msgSend_stret as *const ());
        let mut out = NSRect {
            origin: NSPoint { x: 0.0, y: 0.0 },
            size: NSSize {
                width: 0.0,
                height: 0.0,
            },
        };
        f(&mut out, obj, selector);
        out
    }
}

unsafe fn msg1_usize_ptr(obj: *mut c_void, selector: *mut c_void, arg: usize) -> *mut c_void {
    let f: extern "C" fn(*mut c_void, *mut c_void, usize) -> *mut c_void =
        std::mem::transmute(objc_msgSend as *const ());
    f(obj, selector, arg)
}

unsafe fn msg1_bool(obj: *mut c_void, selector: *mut c_void, arg: i8) {
    let f: extern "C" fn(*mut c_void, *mut c_void, i8) =
        std::mem::transmute(objc_msgSend as *const ());
    f(obj, selector, arg);
}

unsafe fn msg1_ptr(obj: *mut c_void, selector: *mut c_void, arg: *mut c_void) {
    let f: extern "C" fn(*mut c_void, *mut c_void, *mut c_void) =
        std::mem::transmute(objc_msgSend as *const ());
    f(obj, selector, arg);
}

unsafe fn msg1_usize(obj: *mut c_void, selector: *mut c_void, arg: usize) {
    let f: extern "C" fn(*mut c_void, *mut c_void, usize) =
        std::mem::transmute(objc_msgSend as *const ());
    f(obj, selector, arg);
}

unsafe fn msg1_isize(obj: *mut c_void, selector: *mut c_void, arg: isize) {
    let f: extern "C" fn(*mut c_void, *mut c_void, isize) =
        std::mem::transmute(objc_msgSend as *const ());
    f(obj, selector, arg);
}

unsafe fn msg1_rect_ptr(obj: *mut c_void, selector: *mut c_void, rect: NSRect) -> *mut c_void {
    let f: extern "C" fn(*mut c_void, *mut c_void, NSRect) -> *mut c_void =
        std::mem::transmute(objc_msgSend as *const ());
    f(obj, selector, rect)
}

unsafe fn msg3_add_subview(
    superview: *mut c_void,
    selector: *mut c_void,
    subview: *mut c_void,
    positioned: isize,
    relative_to: *mut c_void,
) {
    let f: extern "C" fn(*mut c_void, *mut c_void, *mut c_void, isize, *mut c_void) =
        std::mem::transmute(objc_msgSend as *const ());
    f(superview, selector, subview, positioned, relative_to);
}

/// Attach a behind-window `NSVisualEffectView` to the main `ctdozlite` window once.
pub(super) fn install_vibrancy_if_needed(dark_mode: bool) {
    if VIBRANCY_WINDOW.load(Ordering::Relaxed) != 0 {
        return;
    }
    // SAFETY: Main GUI thread after AppKit init; selectors match ObjC ABI and pointers are null-checked.
    unsafe {
        let ns_app_cls = cls(b"NSApplication\0");
        let ns_color_cls = cls(b"NSColor\0");
        let effect_cls = cls(b"NSVisualEffectView\0");
        if ns_app_cls.is_null() || ns_color_cls.is_null() || effect_cls.is_null() {
            return;
        }
        let app = msg0_ptr(ns_app_cls, sel(b"sharedApplication\0"));
        if app.is_null() {
            return;
        }
        let windows = msg0_ptr(app, sel(b"windows\0"));
        if windows.is_null() {
            return;
        }
        let count = msg0_usize(windows, sel(b"count\0"));
        let sel_obj_at = sel(b"objectAtIndex:\0");
        let sel_title = sel(b"title\0");
        let sel_utf8 = sel(b"UTF8String\0");
        let sel_content_view = sel(b"contentView\0");
        let sel_superview = sel(b"superview\0");

        let mut target_win: *mut c_void = std::ptr::null_mut();
        for idx in 0..count {
            let win = msg1_usize_ptr(windows, sel_obj_at, idx);
            if win.is_null() {
                continue;
            }
            let title_ns = msg0_ptr(win, sel_title);
            if title_ns.is_null() {
                continue;
            }
            let title_c = msg0_ptr(title_ns, sel_utf8).cast::<c_char>();
            if title_c.is_null() {
                continue;
            }
            if is_main_window_title(CStr::from_ptr(title_c).to_bytes()) {
                target_win = win;
                break;
            }
        }
        if target_win.is_null() {
            return;
        }

        let content_view = msg0_ptr(target_win, sel_content_view);
        if content_view.is_null() {
            return;
        }
        let superview = msg0_ptr(content_view, sel_superview);
        if superview.is_null() {
            return;
        }

        let clear_color = msg0_ptr(ns_color_cls, sel(b"clearColor\0"));
        msg1_bool(target_win, sel(b"setOpaque:\0"), 0);
        if !clear_color.is_null() {
            msg1_ptr(target_win, sel(b"setBackgroundColor:\0"), clear_color);
        }
        msg1_bool(target_win, sel(b"setHasShadow:\0"), 1);

        let bounds = msg0_rect(superview, sel(b"bounds\0"));
        let alloc = msg0_ptr(effect_cls, sel(b"alloc\0"));
        if alloc.is_null() {
            return;
        }
        let effect = msg1_rect_ptr(alloc, sel(b"initWithFrame:\0"), bounds);
        if effect.is_null() {
            return;
        }

        const NS_VIEW_WIDTH_SIZABLE: usize = 1 << 1;
        const NS_VIEW_HEIGHT_SIZABLE: usize = 1 << 4;
        const NS_VISUAL_EFFECT_BLENDING_BEHIND_WINDOW: isize = 0;
        const NS_VISUAL_EFFECT_STATE_ACTIVE: isize = 1;
        const NS_WINDOW_BELOW: isize = -1;

        msg1_usize(
            effect,
            sel(b"setAutoresizingMask:\0"),
            NS_VIEW_WIDTH_SIZABLE | NS_VIEW_HEIGHT_SIZABLE,
        );
        msg1_isize(
            effect,
            sel(b"setBlendingMode:\0"),
            NS_VISUAL_EFFECT_BLENDING_BEHIND_WINDOW,
        );
        msg1_isize(
            effect,
            sel(b"setMaterial:\0"),
            material_for_theme(dark_mode),
        );
        msg1_isize(
            effect,
            sel(b"setState:\0"),
            NS_VISUAL_EFFECT_STATE_ACTIVE,
        );
        msg3_add_subview(
            superview,
            sel(b"addSubview:positioned:relativeTo:\0"),
            effect,
            NS_WINDOW_BELOW,
            content_view,
        );
        msg0_void(effect, sel(b"release\0"));

        ctdozlite_macos_set_appearance(target_win, effect, if dark_mode { 1 } else { 0 });
        ctdozlite_macos_install_reopen_handler();
        VIBRANCY_VIEW.store(effect as usize, Ordering::Relaxed);
        VIBRANCY_WINDOW.store(target_win as usize, Ordering::Relaxed);
    }
}

/// Update the behind-window vibrancy material when switching between Dark and Light modes.
pub(super) fn sync_vibrancy_theme(dark_mode: bool) {
    let win = VIBRANCY_WINDOW.load(Ordering::Relaxed) as *mut c_void;
    let effect = VIBRANCY_VIEW.load(Ordering::Relaxed) as *mut c_void;
    if effect.is_null() {
        return;
    }
    // SAFETY: Main GUI thread; `effect` stays retained by the window's theme-frame superview.
    unsafe {
        msg1_isize(effect, sel(b"setMaterial:\0"), material_for_theme(dark_mode));
        ctdozlite_macos_set_appearance(win, effect, if dark_mode { 1 } else { 0 });
    }
}
