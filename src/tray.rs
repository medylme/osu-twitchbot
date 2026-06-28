#[cfg_attr(not(feature = "tray"), allow(dead_code))]
#[derive(Debug, Clone)]
pub enum TrayEvent {
    OpenWindow,
    Quit,
}

#[derive(Debug, Clone)]
pub struct TrayStatus {
    pub osu: String,
    pub twitch: String,
}

#[cfg(feature = "tray")]
pub use imp::spawn;

#[cfg(feature = "tray")]
mod imp {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::time::Duration;

    use tao::event::Event;
    use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder};
    #[cfg(target_os = "linux")]
    use tao::platform::unix::EventLoopBuilderExtUnix;
    #[cfg(target_os = "windows")]
    use tao::platform::windows::EventLoopBuilderExtWindows;
    use tokio::sync::mpsc;
    use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
    use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

    use super::{TrayEvent, TrayStatus};
    use crate::{VERSION, log_warn};

    enum UserEvent {
        Status(TrayStatus),
        Menu(MenuId),
        Open,
    }

    struct Tray {
        _icon: TrayIcon,
        osu_item: MenuItem,
        twitch_item: MenuItem,
        open_id: MenuId,
        open_logs_id: MenuId,
        quit_id: MenuId,
    }

    /// runs the tray on its own tao event loop thread (which pumps gtk on
    /// linux and win32 messages on windows); returns whether it came up
    pub fn spawn(status_rx: mpsc::Receiver<TrayStatus>, event_tx: mpsc::Sender<TrayEvent>) -> bool {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();

        let spawned = std::thread::Builder::new()
            .name("tray".to_string())
            .spawn(move || run(status_rx, event_tx, ready_tx))
            .is_ok();

        spawned && matches!(ready_rx.recv_timeout(Duration::from_secs(5)), Ok(true))
    }

    fn run(
        mut status_rx: mpsc::Receiver<TrayStatus>,
        event_tx: mpsc::Sender<TrayEvent>,
        ready_tx: std::sync::mpsc::Sender<bool>,
    ) {
        // event loop creation initializes gtk on linux and can panic (e.g. no
        // display); report failure so the window falls back to close-to-quit
        let setup = catch_unwind(AssertUnwindSafe(build));
        let Ok(Ok((event_loop, tray))) = setup else {
            let _ = ready_tx.send(false);
            return;
        };

        let proxy = event_loop.create_proxy();
        {
            let proxy = proxy.clone();
            std::thread::spawn(move || {
                while let Some(status) = status_rx.blocking_recv() {
                    let _ = proxy.send_event(UserEvent::Status(status));
                }
            });
        }
        {
            let proxy = proxy.clone();
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                let _ = proxy.send_event(UserEvent::Menu(e.id));
            }));
        }
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = e
            {
                let _ = proxy.send_event(UserEvent::Open);
            }
        }));

        let _ = ready_tx.send(true);

        // never exits: the process ends via the gui (iced::exit) and takes
        // this thread with it, so ControlFlow::Exit is never needed
        event_loop.run(move |event, _, control_flow| {
            *control_flow = ControlFlow::Wait;
            if let Event::UserEvent(user_event) = event {
                match user_event {
                    UserEvent::Status(status) => {
                        tray.osu_item.set_text(format!("osu!: {}", status.osu));
                        tray.twitch_item
                            .set_text(format!("Twitch: {}", status.twitch));
                    }
                    UserEvent::Menu(id) if id == tray.open_id => {
                        send(&event_tx, TrayEvent::OpenWindow);
                    }
                    UserEvent::Menu(id) if id == tray.open_logs_id => {
                        crate::logging::open_log_dir();
                    }
                    UserEvent::Menu(id) if id == tray.quit_id => {
                        send(&event_tx, TrayEvent::Quit);
                    }
                    UserEvent::Open => {
                        send(&event_tx, TrayEvent::OpenWindow);
                    }
                    UserEvent::Menu(_) => {}
                }
            }
        });
    }

    fn build() -> Result<(EventLoop<UserEvent>, Tray), tray_icon::Error> {
        let event_loop = EventLoopBuilder::<UserEvent>::with_user_event()
            .with_any_thread(true)
            .build();

        let menu = Menu::new();
        let title_item = MenuItem::new(format!("osu! twitchbot v{}", VERSION), false, None);
        let osu_item = MenuItem::new("osu!: Disconnected", false, None);
        let twitch_item = MenuItem::new("Twitch: Disconnected", false, None);
        let open_item = MenuItem::new("Open", true, None);
        let open_logs_item = MenuItem::new("Open Logs Folder", true, None);
        let quit_item = MenuItem::new("Quit", true, None);

        let _ = menu.append(&title_item);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&osu_item);
        let _ = menu.append(&twitch_item);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&open_item);
        let _ = menu.append(&open_logs_item);
        let _ = menu.append(&quit_item);

        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("osu! twitchbot")
            .with_icon(load_icon())
            .build()?;

        Ok((
            event_loop,
            Tray {
                _icon: icon,
                open_id: open_item.id().clone(),
                open_logs_id: open_logs_item.id().clone(),
                quit_id: quit_item.id().clone(),
                osu_item,
                twitch_item,
            },
        ))
    }

    fn send(event_tx: &mpsc::Sender<TrayEvent>, event: TrayEvent) {
        if let Err(e) = event_tx.try_send(event) {
            log_warn!("gui", "Failed to send tray event: {}", e);
        }
    }

    fn load_icon() -> tray_icon::Icon {
        let rgba = image::load_from_memory(include_bytes!("../assets/icon.png"))
            .map(|img| img.into_rgba8());
        match rgba {
            Ok(img) => {
                let (width, height) = img.dimensions();
                tray_icon::Icon::from_rgba(img.into_raw(), width, height).expect("valid rgba image")
            }
            Err(_) => tray_icon::Icon::from_rgba(vec![0u8; 4 * 16 * 16], 16, 16)
                .expect("blank icon is valid rgba"),
        }
    }
}
