//! One native camera/encoder per call, with ordered bounded queues for its legs.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, LazyLock, Mutex, Weak,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::{camera, codec::VideoEncoder};

static CURRENT_CALL: LazyLock<Mutex<Weak<VideoSession>>> = LazyLock::new(Default::default);
const QUEUE_CAPACITY: usize = 4;

#[derive(Default)]
struct Slot {
    frames: VecDeque<Vec<u8>>,
    waiting_idr: bool,
    enabled: Weak<AtomicBool>,
}

#[derive(Default)]
struct Fanout {
    next_id: u64,
    slots: BTreeMap<u64, Slot>,
    need_idr: bool,
}

impl Fanout {
    fn subscribe(&mut self, enabled: &Arc<AtomicBool>) -> u64 {
        self.next_id += 1;
        self.slots.insert(
            self.next_id,
            Slot {
                enabled: Arc::downgrade(enabled),
                waiting_idr: true,
                ..Default::default()
            },
        );
        self.need_idr = true;
        self.next_id
    }

    fn invalidate(&mut self) {
        self.need_idr = true;
        for slot in self.slots.values_mut() {
            slot.frames.clear();
            slot.waiting_idr = true;
        }
    }

    fn publish(&mut self, au: Vec<u8>, idr: bool) {
        for slot in self.slots.values_mut() {
            if !slot
                .enabled
                .upgrade()
                .is_some_and(|enabled| enabled.load(Ordering::Acquire))
            {
                slot.frames.clear();
                slot.waiting_idr = true;
                continue;
            }
            if slot.frames.len() >= QUEUE_CAPACITY {
                slot.frames.clear();
                slot.waiting_idr = true;
                self.need_idr = true;
            }
            if slot.waiting_idr && !idr {
                continue;
            }
            slot.waiting_idr = false;
            slot.frames.push_back(au.clone());
        }
    }
}

/// Shared native-video resources for all legs of the application's active call.
pub struct VideoSession {
    /// GUI call generation used to reject delayed frames from an ended call.
    pub generation: u64,
    camera_enabled: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    bus: Arc<Mutex<Fanout>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    gui: Option<crate::gui::GuiTx>,
}

impl VideoSession {
    /// Reuse the active call's camera and encoder, or start its supervisor.
    pub fn acquire(gui: Option<crate::gui::GuiTx>, primary: bool, enabled: bool) -> Arc<Self> {
        let mut current = CURRENT_CALL
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(session) = current.upgrade() {
            if !primary && session.gui.as_ref().is_none_or(|gui| gui.is_current()) {
                return session;
            }
            session.stop.store(true, Ordering::Release);
            if let Ok(mut worker) = session.worker.lock() {
                if let Some(worker) = worker.take() {
                    let _ = worker.join();
                }
            }
        }
        let generation = gui.as_ref().map_or(0, |tx| tx.begin_video_generation());
        let stop = Arc::new(AtomicBool::new(false));
        let bus = Arc::new(Mutex::new(Fanout::default()));
        let stop_worker = stop.clone();
        let bus_worker = bus.clone();
        let gui_worker = gui.clone();
        let worker =
            thread::spawn(move || supervise(stop_worker, bus_worker, gui_worker, generation));
        let session = Arc::new(Self {
            generation,
            camera_enabled: Arc::new(AtomicBool::new(enabled)),
            stop,
            bus,
            worker: Mutex::new(Some(worker)),
            gui,
        });
        *current = Arc::downgrade(&session);
        session
    }

    /// All legs share the user's current camera permission, including late joiners.
    pub fn camera_enabled(&self) -> Arc<AtomicBool> {
        self.camera_enabled.clone()
    }

    /// Changing capture permission invalidates every leg's reference chain immediately.
    pub fn set_camera_enabled(&self, enabled: bool) {
        let mut bus = self.bus.lock().unwrap_or_else(|error| error.into_inner());
        self.camera_enabled.store(enabled, Ordering::Release);
        bus.invalidate();
    }

    /// Register an outgoing leg that resumes only at a newly generated IDR.
    pub fn subscribe(self: &Arc<Self>, enabled: &Arc<AtomicBool>) -> VideoSubscription {
        let id = self
            .bus
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .subscribe(enabled);
        VideoSubscription {
            session: self.clone(),
            id,
        }
    }
}

impl Drop for VideoSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Ok(worker) = self.worker.get_mut() {
            if let Some(worker) = worker.take() {
                let _ = worker.join();
            }
        }
        if let Some(gui) = &self.gui {
            gui.end_video_generation(self.generation);
        }
    }
}

/// An ordered, four-access-unit outgoing queue belonging to one call leg.
pub struct VideoSubscription {
    session: Arc<VideoSession>,
    id: u64,
}

impl VideoSubscription {
    /// Read the next encoded frame without blocking the transport thread.
    pub fn try_recv(&self) -> Option<Vec<u8>> {
        self.session
            .bus
            .lock()
            .ok()?
            .slots
            .get_mut(&self.id)?
            .frames
            .pop_front()
    }

    /// Ask the shared encoder for fresh SPS/PPS and IDR on its next raw frame.
    pub fn request_keyframe(&self) {
        if let Ok(mut bus) = self.session.bus.lock() {
            bus.need_idr = true;
        }
    }
}

impl Drop for VideoSubscription {
    fn drop(&mut self) {
        if let Ok(mut bus) = self.session.bus.lock() {
            bus.slots.remove(&self.id);
        }
    }
}

fn supervise(
    stop: Arc<AtomicBool>,
    bus: Arc<Mutex<Fanout>>,
    gui: Option<crate::gui::GuiTx>,
    generation: u64,
) {
    let mut camera: Option<camera::CameraProcess> = None;
    let mut encoder = None;
    let mut selected = String::new();
    let mut last_frame = Instant::now();
    let mut next_frame = Instant::now();
    let mut preview_visible = false;
    let mut received_frame = false;
    while !stop.load(Ordering::Acquire) && gui.as_ref().is_none_or(|gui| gui.is_current()) {
        let enabled = bus.lock().is_ok_and(|bus| {
            bus.slots.values().any(|slot| {
                slot.enabled
                    .upgrade()
                    .is_some_and(|flag| flag.load(Ordering::Acquire))
            })
        });
        if !enabled {
            if let Some(mut process) = camera.take() {
                let _ = process.stop();
            }
            encoder = None;
            if let Ok(mut bus) = bus.lock() {
                bus.invalidate();
            }
            if preview_visible {
                if let Some(gui) = &gui {
                    gui.clear_video_frame(generation, "call_local_video");
                }
                preview_visible = false;
            }
            thread::sleep(Duration::from_millis(50));
            continue;
        }
        let requested = crate::media::selected_camera_device();
        if camera.is_none() || requested != selected {
            if let Some(mut process) = camera.take() {
                let _ = process.stop();
            }
            if let Ok(mut bus) = bus.lock() {
                bus.invalidate();
            }
            let result = (|| -> anyhow::Result<_> {
                let devices = crate::media::cached_camera_devices();
                let selection = devices
                    .iter()
                    .find(|device| format!("{} [{}]", device.label, device.id) == requested)
                    .map(|device| device.id.as_str())
                    .unwrap_or(&requested);
                let id = camera::resolve_device(&devices, selection)?;
                Ok((camera::CameraProcess::spawn(&id)?, VideoEncoder::new()?))
            })();
            match result {
                Ok((process, codec)) => {
                    camera = Some(process);
                    encoder = Some(codec);
                    selected = requested;
                    last_frame = Instant::now();
                    next_frame = Instant::now();
                    received_frame = false;
                }
                Err(error) => {
                    camera_failure(&bus, &gui, error);
                    continue;
                }
            }
        }
        if Instant::now() < next_frame {
            thread::sleep(Duration::from_millis(5));
            continue;
        }
        next_frame = Instant::now() + Duration::from_millis(67);
        let result = camera
            .as_mut()
            .map(|camera| camera.poll_frame(Duration::from_millis(20)));
        let frame = match result {
            Some(Ok(Some(frame))) => {
                last_frame = Instant::now();
                received_frame = true;
                frame
            }
            // Permission prompts may need user interaction; cancellation still kills the helper.
            Some(Ok(None))
                if last_frame.elapsed()
                    < Duration::from_secs(if received_frame { 5 } else { 120 }) =>
            {
                continue
            }
            Some(Err(error)) => {
                camera_failure(&bus, &gui, error);
                continue;
            }
            _ => {
                camera_failure(
                    &bus,
                    &gui,
                    anyhow::anyhow!(
                        "Camera không trả về hình ảnh; hãy kiểm tra quyền truy cập và kết nối."
                    ),
                );
                continue;
            }
        };
        if let Some(gui) = &gui {
            if let Some(image) =
                image::RgbImage::from_raw(frame.width, frame.height, frame.rgb.clone())
            {
                let preview = image::DynamicImage::ImageRgb8(image)
                    .thumbnail(240, 180)
                    .to_rgba8();
                gui.send_video_frame(
                    generation,
                    "call_local_video",
                    eframe::egui::ColorImage::from_rgba_unmultiplied(
                        [preview.width() as usize, preview.height() as usize],
                        preview.as_raw(),
                    ),
                );
                preview_visible = true;
            }
        }
        let force_idr = bus
            .lock()
            .map(|mut bus| std::mem::take(&mut bus.need_idr))
            .unwrap_or(true);
        let result = encoder.as_mut().map(|encoder: &mut VideoEncoder| {
            encoder.encode_rgb(&frame.rgb, frame.width, frame.height, force_idr)
        });
        match result {
            Some(Ok(au)) if !au.is_empty() => {
                let idr = au
                    .windows(4)
                    .any(|bytes| bytes[..3] == [0, 0, 1] && bytes[3] & 0x1f == 5);
                if let Ok(mut bus) = bus.lock() {
                    bus.publish(au, idr);
                }
            }
            Some(Ok(_)) => {}
            Some(Err(error)) => camera_failure(&bus, &gui, error),
            None => {}
        }
    }
    if let Some(mut process) = camera {
        let _ = process.stop();
    }
}

fn camera_failure(bus: &Mutex<Fanout>, gui: &Option<crate::gui::GuiTx>, error: anyhow::Error) {
    crate::voip_log::line("VCAM", format!("native camera failed: {error}"));
    if let Ok(mut bus) = bus.lock() {
        bus.invalidate();
        for slot in bus.slots.values() {
            if let Some(flag) = slot.enabled.upgrade() {
                flag.store(false, Ordering::Release);
            }
        }
    }
    if let Some(gui) = gui {
        let _ = gui.send(crate::gui::GuiEvent::StatusUpdate(format!(
            "Không thể bật camera: {error}"
        )));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_video_leg_inherits_camera_off_permission() {
        let primary = VideoSession::acquire(None, true, false);
        assert!(!primary.camera_enabled().load(Ordering::Acquire));
        primary.set_camera_enabled(true);
        assert!(primary.camera_enabled().load(Ordering::Acquire));
        primary.set_camera_enabled(false);
        let late_leg = VideoSession::acquire(None, false, true);
        assert!(Arc::ptr_eq(&primary, &late_leg));
        assert!(!late_leg.camera_enabled().load(Ordering::Acquire));
        late_leg.set_camera_enabled(true);
        assert!(primary.camera_enabled().load(Ordering::Acquire));
    }

    #[test]
    fn overflow_waits_for_fresh_idr_without_skipping_other_legs() {
        let enabled = Arc::new(AtomicBool::new(true));
        let mut bus = Fanout::default();
        let slow = bus.subscribe(&enabled);
        let fast = bus.subscribe(&enabled);
        bus.publish(vec![1], true);
        assert_eq!(
            bus.slots.get_mut(&fast).unwrap().frames.pop_front(),
            Some(vec![1])
        );
        for n in 2..=5 {
            bus.publish(vec![n], false);
            assert_eq!(
                bus.slots.get_mut(&fast).unwrap().frames.pop_front(),
                Some(vec![n])
            );
        }
        assert!(bus.slots[&slow].frames.is_empty());
        assert!(bus.slots[&slow].waiting_idr);
        assert!(bus.need_idr);
        bus.publish(vec![6], true);
        assert_eq!(bus.slots[&slow].frames.front(), Some(&vec![6]));
    }

    #[test]
    fn new_leg_does_not_receive_cached_reference_frames() {
        let enabled = Arc::new(AtomicBool::new(true));
        let mut bus = Fanout::default();
        bus.subscribe(&enabled);
        bus.publish(vec![1], true);
        let new = bus.subscribe(&enabled);
        bus.publish(vec![2], false);
        assert!(bus.slots[&new].frames.is_empty());
        bus.publish(vec![3], true);
        assert_eq!(bus.slots[&new].frames.front(), Some(&vec![3]));
    }
}
