use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use ksni::{
    menu::{CheckmarkItem, MenuItem, StandardItem},
    Tray, TrayService,
};

pub struct TrayState {
    pub close_to_tray: AtomicBool,
    pub restore_requested: AtomicBool,
}

struct AppTray {
    state: Arc<TrayState>,
}

impl Tray for AppTray {
    fn id(&self) -> String {
        "ctdozlite".into()
    }

    fn title(&self) -> String {
        "ctdozlite".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Communications
    }

    fn icon_name(&self) -> String {
        "ctdozlite".into()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.state.restore_requested.store(true, Ordering::Release);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            CheckmarkItem {
                label: "Đóng vào khay hệ thống".into(),
                checked: self.state.close_to_tray.load(Ordering::Acquire),
                activate: Box::new(|tray| {
                    let enabled = !tray.state.close_to_tray.load(Ordering::Acquire);
                    tray.state.close_to_tray.store(enabled, Ordering::Release);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Thoát ctdozlite".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub fn start() -> Arc<TrayState> {
    let state = Arc::new(TrayState {
        close_to_tray: AtomicBool::new(false),
        restore_requested: AtomicBool::new(false),
    });
    TrayService::new(AppTray {
        state: Arc::clone(&state),
    })
    .spawn();
    state
}
