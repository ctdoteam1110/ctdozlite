//! Shared animation helpers: ease curves, the macOS "Reduce motion" setting and the
//! `appear` fade/slide-in used by popups. (The original Lucian #001 glass send button that lived
//! here was replaced by a plain round send button.)

use super::*;

/// Send progress: idle, request in flight, done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GlassState {
    Idle,
    Load,
    Ok,
}

/// Ease-out curve used by `appear`.
const EASE: [f32; 4] = [0.2, 0.8, 0.2, 1.0];

/// CSS `cubic-bezier(x1,y1,x2,y2)` at progress `t` (Newton on x, then y).
fn bezier(c: [f32; 4], t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let [x1, y1, x2, y2] = c;
    let bx = |s: f32| 3.0 * (1.0 - s).powi(2) * s * x1 + 3.0 * (1.0 - s) * s * s * x2 + s.powi(3);
    let by = |s: f32| 3.0 * (1.0 - s).powi(2) * s * y1 + 3.0 * (1.0 - s) * s * s * y2 + s.powi(3);
    let mut s = t;
    for _ in 0..8 {
        let dx = 3.0 * (1.0 - s).powi(2) * x1
            + 6.0 * (1.0 - s) * s * (x2 - x1)
            + 3.0 * s * s * (1.0 - x2);
        if dx.abs() < 1e-5 {
            break;
        }
        s = (s - (bx(s) - t) / dx).clamp(0.0, 1.0);
    }
    by(s)
}

/// macOS "Reduce motion" (System Settings → Accessibility → Display), read once.
pub(super) fn reduce_motion() -> bool {
    static RM: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *RM.get_or_init(|| {
        #[cfg(target_os = "macos")]
        {
            // SAFETY: read-only property on the shared workspace.
            unsafe {
                objc2_app_kit::NSWorkspace::sharedWorkspace()
                    .accessibilityDisplayShouldReduceMotion()
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    })
}

/// Appear animation progress (0 → 1, ease-out, `secs` long) for something drawn this frame.
/// Restarts automatically when the element was not drawn on the previous frame, so callers
/// need no open/close bookkeeping. Instant under macOS "Reduce motion".
pub(super) fn appear(ctx: &egui::Context, id: Id, secs: f32) -> f32 {
    if reduce_motion() || secs <= 0.0 {
        return 1.0;
    }
    let now = ctx.input(|i| i.time);
    let frame = ctx.cumulative_pass_nr();
    let key = id.with("appear_anim");
    let (start, last) = ctx
        .data(|d| d.get_temp::<(f64, u64)>(key))
        .unwrap_or((now, 0));
    let start = if last + 1 < frame { now } else { start };
    ctx.data_mut(|d| d.insert_temp(key, (start, frame)));
    let t = (((now - start) as f32) / secs).clamp(0.0, 1.0);
    if t < 1.0 {
        ctx.request_repaint();
    }
    bezier(EASE, t)
}

#[cfg(test)]
mod tests {
    use super::bezier;

    #[test]
    fn bezier_matches_css_endpoints_and_spring_overshoot() {
        assert!((bezier([0.2, 0.8, 0.2, 1.0], 0.0)).abs() < 1e-4);
        assert!((bezier([0.2, 0.8, 0.2, 1.0], 1.0) - 1.0).abs() < 1e-4);
        // cubic-bezier(.34,1.45,.64,1) overshoots past 1 mid-way.
        let peak = (1..100)
            .map(|i| bezier([0.34, 1.45, 0.64, 1.0], i as f32 / 100.0))
            .fold(0.0, f32::max);
        assert!(peak > 1.02, "spring should overshoot, got {peak}");
    }
}
