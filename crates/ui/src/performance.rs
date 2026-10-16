mod cpu;
mod disk;
mod gpu;
mod memory;
mod network;

use std::collections::HashMap;

use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Element, Fill, Task};
use peepers_core::cpu::Info;
use peepers_core::disk::Kind as Drive;
use peepers_core::health::{self, Health};
use peepers_core::network::Kind;
use peepers_core::telemetry::Vitals;

use crate::blocking;
use crate::graph::Graph;
use crate::history::History;
use crate::settings::Temperature;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Device {
    Cpu,
    Memory,
    Disk(String),
    Network(String),
    Gpu(String),
}

impl Device {
    fn card(&self, history: &History) -> Card {
        match self {
            Self::Cpu => cpu::card(history),
            Self::Memory => memory::card(history),
            Self::Disk(name) => disk::card(history, name),
            Self::Network(name) => network::card(history, name),
            Self::Gpu(name) => gpu::card(history, name),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Select(Device),
    Health(String, Result<Health, String>),
}

/// What the device list shows for one device.
struct Card {
    title: String,
    value: String,
    graph: Graph,
}

impl Card {
    fn view<'a, Message: 'a>(self) -> Element<'a, Message> {
        column![
            row![
                container(text(self.title).wrapping(Wrapping::None))
                    .width(Fill)
                    .clip(true),
                text(self.value).wrapping(Wrapping::None),
            ]
            .spacing(8),
            container(self.graph.view()).height(40),
        ]
        .spacing(6)
        .into()
    }
}

fn waiting<'a, Message: 'a>() -> Element<'a, Message> {
    container(text("Waiting for the first sample").style(text::secondary))
        .center(Fill)
        .into()
}

/// Every device's card in a column, for a window that only watches.
pub fn glance<'a, Message: 'a>(history: &History) -> Element<'a, Message> {
    let Some(latest) = history.latest() else {
        return waiting();
    };
    scrollable(
        column(
            devices(latest)
                .iter()
                .map(|device| device.card(history).view()),
        )
        .spacing(16)
        .padding(12),
    )
    .into()
}

pub struct Performance {
    selected: Device,
    cpu: Info,
    /// Each disk's health as last read, or why it could not be.
    health: HashMap<String, Result<Health, String>>,
}

impl Default for Performance {
    fn default() -> Self {
        Self {
            selected: Device::Cpu,
            cpu: Info::read(),
            health: HashMap::new(),
        }
    }
}

impl Performance {
    pub fn update(&mut self, message: Message, history: &History) -> Task<Message> {
        match message {
            Message::Select(device) => {
                let task = match &device {
                    Device::Disk(name) => {
                        disk_kind(history, name).map_or_else(Task::none, |kind| {
                            let name = name.clone();
                            blocking::run(move || {
                                let result = health::read(&name, kind);
                                (name, result)
                            })
                            .map(|(name, result)| Message::Health(name, result))
                        })
                    }
                    _ => Task::none(),
                };
                self.selected = device;
                task
            }
            Message::Health(name, result) => {
                self.health.insert(name, result);
                Task::none()
            }
        }
    }

    pub fn view<'a>(&'a self, history: &History, unit: Temperature) -> Element<'a, Message> {
        let Some(latest) = history.latest() else {
            return waiting();
        };
        let devices = devices(latest);
        let selected = if devices.contains(&self.selected) {
            &self.selected
        } else {
            &Device::Cpu
        };

        let list = devices.iter().map(|device| {
            button(device.card(history).view())
                .width(Fill)
                .on_press(Message::Select(device.clone()))
                .style(if device == selected {
                    button::secondary
                } else {
                    button::text
                })
                .into()
        });

        row![
            scrollable(column(list).spacing(4).padding(8)).width(260),
            container(self.detail(selected, history, latest, unit)).width(Fill),
        ]
        .into()
    }

    fn detail<'a>(
        &'a self,
        device: &Device,
        history: &History,
        latest: &Vitals,
        unit: Temperature,
    ) -> Element<'a, Message> {
        match device {
            Device::Cpu => cpu::view(history, latest, &self.cpu),
            Device::Memory => memory::view(history, latest),
            Device::Disk(name) => disk::view(history, latest, name, unit, self.health.get(name)),
            Device::Network(name) => network::view(history, latest, name),
            Device::Gpu(name) => gpu::view(history, latest, name, unit),
        }
    }
}

fn disk_kind(history: &History, name: &str) -> Option<Drive> {
    history
        .latest()?
        .disks
        .iter()
        .find(|disk| disk.name == name)
        .map(|disk| disk.kind)
}

fn devices(vitals: &Vitals) -> Vec<Device> {
    [Device::Cpu, Device::Memory]
        .into_iter()
        .chain(
            vitals
                .disks
                .iter()
                .map(|disk| Device::Disk(disk.name.clone())),
        )
        .chain(
            vitals
                .interfaces
                .iter()
                .filter(|interface| matches!(interface.kind, Kind::Ethernet | Kind::Wireless))
                .map(|interface| Device::Network(interface.name.clone())),
        )
        .chain(
            vitals
                .adapters
                .iter()
                .map(|adapter| Device::Gpu(adapter.card.clone())),
        )
        .collect()
}
