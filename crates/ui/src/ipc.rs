use std::sync::Arc;

use iced::futures::SinkExt;
use iced::{Subscription, Task, stream};
use peepers_core::ipc::{Command, Listener};

use crate::blocking;

#[derive(Debug, Clone)]
pub enum Message {
    Started(Result<Arc<Listener>, String>),
    Received(Command),
}

/// The application's door for other processes, open for as long as it holds the bus name.
#[derive(Debug, Default)]
pub struct Door {
    listener: Option<Arc<Listener>>,
}

impl Door {
    pub fn open() -> Task<Message> {
        blocking::run(|| Listener::start().map(Arc::new)).map(Message::Started)
    }

    /// A copy that cannot claim the name is a second one, and the first keeps answering.
    pub fn started(&mut self, result: Result<Arc<Listener>, String>) {
        self.listener = result.ok();
    }

    pub fn subscription(&self) -> Subscription<Message> {
        self.listener
            .as_ref()
            .map_or_else(Subscription::none, |listener| {
                Subscription::run_with(listener.commands(), |commands| {
                    let commands = commands.clone();
                    stream::channel(1, async move |mut output| {
                        loop {
                            let waiting = commands.clone();
                            let Some(command) = smol::unblock(move || waiting.recv()).await else {
                                break;
                            };
                            if output.send(Message::Received(command)).await.is_err() {
                                break;
                            }
                        }
                    })
                })
            })
    }
}
