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

## When to remove this

Delete the `vendor/` directory and the `[patch.crates-io]` entry once the
`eframe`/`egui-winit` version we depend on handles IME on Linux itself. If a
`eframe` upgrade makes cargo warn that the patch "was not used", re-check
whether the fix is still needed before re-applying it to the new version.
