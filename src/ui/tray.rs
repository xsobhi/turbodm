//! Tray icon (StatusNotifierItem, shown by Cinnamon/KDE/most panels).

use super::Ctx;
use gtk::glib;
use gtk::prelude::*;
use ksni::menu::StandardItem;
use ksni::TrayMethods;
use std::rc::Rc;

#[derive(Clone, Copy)]
enum Cmd {
    Available(bool), // did a tray host accept our icon?
    Show,
    PauseAll,
    ResumeAll,
    Quit,
}

struct Tray {
    tx: async_channel::Sender<Cmd>,
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "turbodm".into()
    }
    fn title(&self) -> String {
        "TurboDM".into()
    }
    fn icon_name(&self) -> String {
        "turbodm".into()
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.tx.try_send(Cmd::Show);
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let item = |label: &str, cmd: Cmd| -> ksni::MenuItem<Self> {
            StandardItem {
                label: label.into(),
                activate: Box::new(move |tray: &mut Self| {
                    let _ = tray.tx.try_send(cmd);
                }),
                ..Default::default()
            }
            .into()
        };
        vec![
            item("Show TurboDM", Cmd::Show),
            item("Resume all", Cmd::ResumeAll),
            item("Pause all", Cmd::PauseAll),
            ksni::MenuItem::Separator,
            item("Quit", Cmd::Quit),
        ]
    }
}

pub fn start(ctx: &Rc<Ctx>) {
    let (tx, rx) = async_channel::unbounded();
    ctx.rt.spawn(async move {
        let status = tx.clone();
        match (Tray { tx }).spawn().await {
            Ok(_handle) => {
                let _ = status.send(Cmd::Available(true)).await;
                std::future::pending::<()>().await // keep the icon alive
            }
            Err(_) => {
                // no tray on this panel: closing the window will minimize instead
                let _ = status.send(Cmd::Available(false)).await;
            }
        }
    });
    let ctx = ctx.clone();
    glib::spawn_future_local(async move {
        while let Ok(cmd) = rx.recv().await {
            match cmd {
                Cmd::Available(ok) => ctx.tray.set(ok),
                Cmd::Show => ctx.show(),
                Cmd::PauseAll => ctx.manager.pause_all(),
                Cmd::ResumeAll => ctx.manager.resume_all(),
                Cmd::Quit => ctx.app.quit(),
            }
        }
    });
}
