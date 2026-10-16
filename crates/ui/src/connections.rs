use std::collections::HashMap;
use std::time::Duration;

use iced::time;
use iced::widget::{button, checkbox, column, pick_list, row, space, table, text, text_input};
use iced::{Element, Fill, Subscription, Task};
use peepers_core::connections::{self, Connection, Endpoint, Exposure, Protocol, State};

use crate::blocking;
use crate::processes::Processes;
use crate::widgets::{scrolling, stats};

const REFRESH: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    All,
    Listening,
    PublicListeners,
}

impl Scope {
    const ALL: [Self; 3] = [Self::All, Self::Listening, Self::PublicListeners];
}

impl std::fmt::Display for Scope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::All => "All connections",
            Self::Listening => "Listening ports",
            Self::PublicListeners => "Ports open to every interface or the internet",
        })
    }
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    connections: Vec<Connection>,
    owners: HashMap<u64, u32>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Refresh,
    Loaded(Snapshot),
    Scope(Scope),
    Filter(String),
    UnixSockets(bool),
    Select(Connection),
}

#[derive(Debug, Default)]
pub struct Connections {
    snapshot: Option<Snapshot>,
    scope: Scope,
    filter: String,
    unix: bool,
    selected: Option<Connection>,
}

impl Connections {
    pub fn enter() -> Task<Message> {
        blocking::run(load).map(Message::Loaded)
    }

    pub fn subscription() -> Subscription<Message> {
        time::every(REFRESH).map(|_| Message::Refresh)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Refresh => return Self::enter(),
            Message::Loaded(snapshot) => self.snapshot = Some(snapshot),
            Message::Scope(scope) => self.scope = scope,
            Message::Filter(filter) => self.filter = filter,
            Message::UnixSockets(unix) => self.unix = unix,
            Message::Select(connection) => self.selected = Some(connection),
        }
        Task::none()
    }

    pub fn view<'a>(&'a self, processes: &'a Processes) -> Element<'a, Message> {
        let Some(snapshot) = &self.snapshot else {
            return text("Reading the socket tables")
                .style(text::secondary)
                .into();
        };
        let needle = self.filter.to_lowercase();
        let owner = |connection: &Connection| {
            snapshot
                .owners
                .get(&connection.inode)
                .map(|&pid| (pid, processes.name(pid).unwrap_or("?").to_owned()))
        };
        let rows: Vec<Row> = snapshot
            .connections
            .iter()
            .filter(|connection| self.unix || !is_unix(connection.protocol))
            .filter(|connection| in_scope(self.scope, connection))
            .map(|connection| Row {
                connection,
                owner: owner(connection),
            })
            .filter(|row| row.matches(&needle))
            .collect();
        let count = rows.len();

        let selected = self.selected.as_ref();
        let columns = [
            table::column("Process", move |row: Row| process_cell(&row, selected)).width(Fill),
            table::column("Protocol", |row: Row| {
                text(protocol(row.connection.protocol))
            }),
            table::column("Local endpoint", |row: Row| {
                text(endpoint(&row.connection.local))
            }),
            table::column("Remote endpoint", |row: Row| {
                text(endpoint(&row.connection.remote))
            }),
            table::column("State", |row: Row| {
                text(state(row.connection.protocol, row.connection.state))
            }),
            table::column("Exposure", |row: Row| {
                text(exposure(row.connection).unwrap_or_default())
            }),
        ];

        let toolbar = row![
            pick_list(Scope::ALL, Some(self.scope), Message::Scope),
            text_input("Filter by process, port or address", &self.filter)
                .on_input(Message::Filter)
                .width(320),
            checkbox(self.unix)
                .label("UNIX sockets")
                .on_toggle(Message::UnixSockets),
            space::horizontal(),
            text(format!("{count} connections")).style(text::secondary),
        ]
        .spacing(16)
        .padding(8)
        .align_y(iced::Center);

        column![
            toolbar,
            scrolling(table(columns, rows).padding_x(10).padding_y(2))
                .width(Fill)
                .height(Fill)
        ]
        .extend(
            self.selected
                .as_ref()
                .map(|connection| route(connection, snapshot, processes)),
        )
        .into()
    }
}

#[derive(Clone)]
struct Row<'a> {
    connection: &'a Connection,
    owner: Option<(u32, String)>,
}

impl Row<'_> {
    fn matches(&self, needle: &str) -> bool {
        needle.is_empty()
            || self.owner.as_ref().is_some_and(|(pid, name)| {
                name.to_lowercase().contains(needle) || pid.to_string() == needle
            })
            || endpoint(&self.connection.local)
                .to_lowercase()
                .contains(needle)
            || endpoint(&self.connection.remote)
                .to_lowercase()
                .contains(needle)
    }
}

pub fn load() -> Snapshot {
    Snapshot {
        connections: connections::read().unwrap_or_default(),
        owners: connections::owners(),
    }
}

fn is_unix(protocol: Protocol) -> bool {
    matches!(
        protocol,
        Protocol::UnixStream | Protocol::UnixDatagram | Protocol::UnixSeqPacket
    )
}

fn in_scope(scope: Scope, connection: &Connection) -> bool {
    match scope {
        Scope::All => true,
        Scope::Listening => connection.state == State::Listen,
        Scope::PublicListeners => {
            connection.state == State::Listen
                && connection
                    .exposure()
                    .is_some_and(|exposure| exposure >= Exposure::AllInterfaces)
        }
    }
}

fn process_cell<'a>(row: &Row<'a>, selected: Option<&Connection>) -> Element<'a, Message> {
    let label = row.owner.as_ref().map_or_else(
        || "Not visible".to_owned(),
        |(pid, name)| format!("{name} ({pid})"),
    );
    button(text(label))
        .on_press(Message::Select(row.connection.clone()))
        .padding([0, 6])
        .style(if selected == Some(row.connection) {
            button::primary
        } else {
            button::text
        })
        .into()
}

fn route<'a>(
    connection: &Connection,
    snapshot: &Snapshot,
    processes: &Processes,
) -> Element<'a, Message> {
    let owner = snapshot.owners.get(&connection.inode).map_or_else(
        || "Not visible without privilege".to_owned(),
        |&pid| format!("{} ({pid})", processes.name(pid).unwrap_or("?")),
    );
    stats([
        ("Owner", owner),
        ("Local", endpoint(&connection.local)),
        ("Remote", endpoint(&connection.remote)),
        ("Socket inode", connection.inode.to_string()),
        (
            "User ID",
            connection
                .uid
                .map_or_else(|| "n/a".to_owned(), |uid| uid.to_string()),
        ),
        (
            "Exposure",
            exposure(connection).unwrap_or("Not listening").to_owned(),
        ),
    ])
}

fn endpoint(endpoint: &Endpoint) -> String {
    match endpoint {
        Endpoint::Address(address) if address.port() == 0 && address.ip().is_unspecified() => {
            "-".to_owned()
        }
        Endpoint::Address(address) => address.to_string(),
        Endpoint::Path(path) => path.clone(),
        Endpoint::None => "unnamed".to_owned(),
    }
}

fn protocol(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::Tcp => "TCP",
        Protocol::Udp => "UDP",
        Protocol::UnixStream => "UNIX stream",
        Protocol::UnixDatagram => "UNIX datagram",
        Protocol::UnixSeqPacket => "UNIX packet",
    }
}

/// UDP and UNIX sockets have no handshake, so only the states that mean something there are named.
fn state(protocol: Protocol, state: State) -> &'static str {
    match (protocol, state) {
        (Protocol::Udp, State::Established) => "Connected",
        (Protocol::Udp, State::Closed) => "Bound",
        (_, State::Closed) => "Unconnected",
        (_, State::Established) => "Established",
        (_, State::SynSent) => "SYN sent",
        (_, State::SynReceived) => "SYN received",
        (_, State::FinWait1) => "FIN wait 1",
        (_, State::FinWait2) => "FIN wait 2",
        (_, State::TimeWait) => "Time wait",
        (_, State::CloseWait) => "Close wait",
        (_, State::LastAck) => "Last ACK",
        (_, State::Listen) => "Listening",
        (_, State::Closing) => "Closing",
    }
}

/// Only a socket that accepts traffic has an exposure; an outbound connection has none to speak of.
fn exposure(connection: &Connection) -> Option<&'static str> {
    let accepting = connection.state == State::Listen
        || (connection.protocol == Protocol::Udp && connection.state == State::Closed);
    accepting.then(|| match connection.exposure() {
        None => "Local IPC",
        Some(Exposure::Loopback) => "This computer only",
        Some(Exposure::LocalNetwork) => "Local network",
        Some(Exposure::AllInterfaces) => "All interfaces",
        Some(Exposure::Public) => "Public address",
    })
}
