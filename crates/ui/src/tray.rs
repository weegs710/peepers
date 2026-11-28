use std::sync::Arc;

use iced::futures::SinkExt;
use iced::{Subscription, Task, stream};
use peepers_core::tray::{Event, Gauges, IconColors, Tray};

use crate::blocking;

#[derive(Debug, Clone)]
pub enum Message {
    Started(Result<Arc<Tray>, String>),
    Clicked(Event),
}

/// The tray icon when it is wanted and the desktop has a tray to show it in.
#[derive(Debug, Default)]
pub struct Indicator {
    tray: Option<Arc<Tray>>,
    error: Option<String>,
}

impl Indicator {
    /// Starts or removes the icon so that it matches `wanted`.
    pub fn sync(&mut self, wanted: bool) -> Task<Message> {
        match (wanted, self.tray.is_some()) {
            (true, false) => blocking::run(|| Tray::start().map(Arc::new)).map(Message::Started),
            (false, _) => {
                self.tray = None;
                self.error = None;
                Task::none()
            }
            (true, true) => Task::none(),
        }
    }

    /// Keeps a started icon only if it is still wanted.
    pub fn started(&mut self, result: Result<Arc<Tray>, String>, wanted: bool) {
        match result {
            Ok(tray) if wanted => {
                self.tray = Some(tray);
                self.error = None;
            }
            Ok(_) => {}
            Err(error) => self.error = Some(error),
        }
    }

    pub fn is_active(&self) -> bool {
        self.tray.is_some()
    }

    /// Why the icon is missing when it was asked for.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn show(&self, gauges: Gauges) {
        if let Some(tray) = &self.tray {
            tray.show(gauges);
        }
    }

    pub fn set_colors(&self, colors: IconColors) {
        if let Some(tray) = &self.tray {
            tray.set_colors(colors);
        }
    }

    pub fn set_window_open(&self, open: bool) {
        if let Some(tray) = &self.tray {
            tray.set_window_open(open);
        }
    }

    pub fn set_popout_open(&self, open: bool) {
        if let Some(tray) = &self.tray {
            tray.set_popout_open(open);
        }
    }

    pub fn set_paused(&self, paused: bool) {
        if let Some(tray) = &self.tray {
            tray.set_paused(paused);
        }
    }

    /// Clicks on the icon and its menu, for as long as the icon is there.
    pub fn subscription(&self) -> Subscription<Message> {
        self.tray.as_ref().map_or_else(Subscription::none, |tray| {
            Subscription::run_with(tray.events(), |events| {
                let events = events.clone();
                stream::channel(1, async move |mut output| {
                    loop {
                        let waiting = events.clone();
                        let Some(event) = smol::unblock(move || waiting.recv()).await else {
                            break;
                        };
                        if output.send(Message::Clicked(event)).await.is_err() {
                            break;
                        }
                    }
                })
            })
        })
    }
}
