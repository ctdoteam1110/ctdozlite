# Local patches to `egui-winit`

This directory is `egui-winit 0.29.1` from crates.io, wired in through
`[patch.crates-io]` in the root `Cargo.toml`. Everything is identical to the
published crate except the files listed here (the crates.io-only
`Cargo.toml.orig` and `.cargo_vcs_info.json` were left out on purpose).

## `src/lib.rs`: forward IME events on Linux

Upstream ignores every IME event on Linux because of
[emilk/egui#5008](https://github.com/emilk/egui/issues/5008): winit sends
`Ime::Enabled` there as a text-input focus notification, egui treats it as
"composition started", and `TextEdit` then drops Backspace, arrow keys and key
repeats until a `Disabled` arrives.

The patch stops treating `Enabled` as composition start on Linux. Composition
now begins at the first non-empty `Preedit` (which already calls
`ime_event_enable()`), so Vietnamese/CJK input methods work without breaking
normal typing. Behaviour on Windows and macOS is unchanged.

The whole change is one arm of the `WindowEvent::Ime` match (the surrounding
`if cfg!(target_os = "linux") { .. } else { .. }` wrapper is removed, which
dedents the block). Ignoring whitespace, the diff against upstream is:

```diff
-                if cfg!(target_os = "linux") {
-                    // We ignore IME events on linux, because of https://github.com/emilk/egui/issues/5008
-                } else {
 ...
                         winit::event::Ime::Enabled => {
-                            self.ime_event_enable();
+                            if !cfg!(target_os = "linux") {
+                                self.ime_event_enable();
+                            }
                         }
```

## `src/lib.rs`: keep the paste key event when the clipboard has no text

Upstream turns `Ctrl+V` / `Shift+Insert` / `Paste` into `Event::Paste(text)`
and returns early, dropping the key event even when the clipboard holds no
text. When the clipboard only holds an image (for example an Ubuntu
screenshot, `image/png` only), the app then sees nothing and cannot read the
bitmap itself. Windows hides this with a `GetAsyncKeyState` fallback in
`is_clipboard_paste_triggered`; Linux and macOS have no such fallback.

The patch returns early only when a non-empty text paste was emitted;
otherwise the normal `Event::Key` is pushed. `TextEdit` does not act on a
bare `Ctrl+V` key event, so text input is unaffected.

```diff
                         if !contents.is_empty() {
                             self.egui_input.events.push(egui::Event::Paste(contents));
+                            return;
                         }
                     }
-                    return;
+                    // No text on the clipboard (e.g. an image): fall through ...
                 }
```

## When to remove this

Delete the `vendor/` directory and the `[patch.crates-io]` entry once the
`eframe`/`egui-winit` version we depend on handles IME on Linux itself. If a
`eframe` upgrade makes cargo warn that the patch "was not used", re-check
whether the fix is still needed before re-applying it to the new version.
