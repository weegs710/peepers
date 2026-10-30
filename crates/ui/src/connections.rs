use std::cmp::Ordering;
use std::collections::HashMap;
use std::time::Duration;

use iced::time;
use iced::widget::{checkbox, column, container, pick_list, row, space, table, text, text_input};
use iced::{Element, Fill, Subscription, Task};
use peepers_core::connections::{self, Connection, Endpoint, Exposure, Protocol, State};

use crate::blocking;
use crate::processes::Processes;
use crate::selectable;
use crate::widgets::{Sortable, Sorting, scrolling, sort_header, stats};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Column {
    #[default]
    Process,
    Protocol,
    LocalEndpoint,
    RemoteEndpoint,
    State,
    Exposure,
}

impl Sortable for Column {
    fn descending_first(self) -> bool {
        false
    }
}

impl Column {
    fn compare(self, a: &Row, b: &Row) -> Ordering {
        match self {
            Self::Process => owner_key(a).cmp(&owner_key(b)),
            Self::Protocol => protocol(a.connection.protocol).cmp(protocol(b.connection.protocol)),
            Self::LocalEndpoint => endpoint_order(&a.connection.local, &b.connection.local),
            Self::RemoteEndpoint => endpoint_order(&a.connection.remote, &b.connection.remote),
            Self::State => state(a.connection.protocol, a.connection.state)
                .cmp(state(b.connection.protocol, b.connection.state)),
            Self::Exposure => reach(a.connection).cmp(&reach(b.connection)),
        }
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
    Sort(Column),
}

#[derive(Debug, Default)]
pub struct Connections {
    snapshot: Option<Snapshot>,
    sorting: Sorting<Column>,
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
            Message::Sort(column) => self.sorting.press(column),
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
        let mut rows: Vec<Row> = snapshot
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
        self.sorting
            .sort(&mut rows, |column, a, b| column.compare(a, b));
        let count = rows.len();
        let header = |label, column| {
            sort_header(label, self.sorting.direction(column), Message::Sort(column))
        };

        let selected = self.selected.as_ref();
        let columns = [
            table::column(header("Process", Column::Process), |row: Row| {
                process_cell(&row)
            })
            .width(Fill),
            table::column(header("Protocol", Column::Protocol), |row: Row| {
                text(protocol(row.connection.protocol))
            }),
            table::column(
                header("Local endpoint", Column::LocalEndpoint),
                |row: Row| text(endpoint(&row.connection.local)),
            ),
            table::column(
                header("Remote endpoint", Column::RemoteEndpoint),
                |row: Row| text(endpoint(&row.connection.remote)),
            ),
            table::column(header("State", Column::State), |row: Row| {
                text(state(row.connection.protocol, row.connection.state))
            }),
            table::column(header("Exposure", Column::Exposure), |row: Row| {
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
            scrolling(selectable::table(
                columns,
                rows,
                |row| Message::Select(row.connection.clone()),
                |row| selected == Some(row.connection),
            ))
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

fn process_cell<'a>(row: &Row<'a>) -> Element<'a, Message> {
    let label = row.owner.as_ref().map_or_else(
        || "Not visible".to_owned(),
        |(pid, name)| format!("{name} ({pid})"),
    );
    container(text(label)).padding([0, 6]).into()
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

/// Whether the socket accepts traffic, which is what gives it an exposure.
fn accepts(connection: &Connection) -> bool {
    connection.state == State::Listen
        || (connection.protocol == Protocol::Udp && connection.state == State::Closed)
}

/// How far a socket reaches, for ordering: sockets that accept nothing first, then by exposure.
fn reach(connection: &Connection) -> (bool, Option<Exposure>) {
    let accepting = accepts(connection);
    (accepting, connection.exposure().filter(|_| accepting))
}

/// The process that owns the socket by name and then number, with sockets that have no visible
/// owner last.
fn owner_key(row: &Row) -> (bool, String, u32) {
    row.owner.as_ref().map_or_else(
        || (true, String::new(), 0),
        |(pid, name)| (false, name.to_lowercase(), *pid),
    )
}

/// Addresses in numeric order, then paths, then sockets without a name.
fn endpoint_order(a: &Endpoint, b: &Endpoint) -> Ordering {
    let rank = |endpoint: &Endpoint| match endpoint {
        Endpoint::Address(_) => 0,
        Endpoint::Path(_) => 1,
        Endpoint::None => 2,
    };
    match (a, b) {
        (Endpoint::Address(a), Endpoint::Address(b)) => a.cmp(b),
        (Endpoint::Path(a), Endpoint::Path(b)) => a.cmp(b),
        _ => rank(a).cmp(&rank(b)),
    }
}

/// Only a socket that accepts traffic has an exposure; an outbound connection has none to speak of.
fn exposure(connection: &Connection) -> Option<&'static str> {
    accepts(connection).then(|| match connection.exposure() {
        None => "Local IPC",
        Some(Exposure::Loopback) => "This computer only",
        Some(Exposure::LocalNetwork) => "Local network",
        Some(Exposure::AllInterfaces) => "All interfaces",
        Some(Exposure::Public) => "Public address",
    })
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr};

    use super::*;
    use crate::widgets::pressed;

    fn address(last: u8, port: u16) -> Endpoint {
        Endpoint::Address(SocketAddr::from((Ipv4Addr::new(10, 0, 0, last), port)))
    }

    fn connection(protocol: Protocol, local: Endpoint, state: State, inode: u64) -> Connection {
        Connection {
            protocol,
            local,
            remote: Endpoint::None,
            state,
            uid: None,
            inode,
        }
    }

    /// Three sockets whose inode says which is which, owned by `sshd`, nobody visible and `Nginx`.
    fn sockets() -> Vec<Connection> {
        vec![
            connection(Protocol::Tcp, address(9, 80), State::Listen, 1),
            connection(Protocol::Udp, address(10, 5), State::Established, 2),
            connection(
                Protocol::UnixStream,
                Endpoint::Path("/run/x".to_owned()),
                State::Closed,
                3,
            ),
        ]
    }

    fn owners() -> HashMap<u64, u32> {
        HashMap::from([(1, 20), (3, 10)])
    }

    fn names(pid: u32) -> &'static str {
        if pid == 10 { "sshd" } else { "Nginx" }
    }

    fn ordered(column: Column, again: bool, sockets: &[Connection]) -> Vec<u64> {
        let mut sorting = Sorting::new(column);
        if again {
            sorting.press(column);
        }
        let owners = owners();
        let mut rows: Vec<Row> = sockets
            .iter()
            .map(|connection| Row {
                connection,
                owner: owners
                    .get(&connection.inode)
                    .map(|&pid| (pid, names(pid).to_owned())),
            })
            .collect();
        sorting.sort(&mut rows, |column, a, b| column.compare(a, b));
        rows.into_iter().map(|row| row.connection.inode).collect()
    }

    #[test]
    fn every_column_orders_the_sockets() {
        let sockets = sockets();
        assert_eq!(ordered(Column::Process, false, &sockets), [1, 3, 2]);
        assert_eq!(ordered(Column::Protocol, false, &sockets), [1, 2, 3]);
        assert_eq!(ordered(Column::LocalEndpoint, false, &sockets), [1, 2, 3]);
        assert_eq!(ordered(Column::RemoteEndpoint, false, &sockets), [1, 2, 3]);
        assert_eq!(ordered(Column::State, false, &sockets), [2, 1, 3]);
        assert_eq!(ordered(Column::Exposure, false, &sockets), [2, 3, 1]);
    }

    #[test]
    fn addresses_order_by_number_and_not_by_their_spelling() {
        let mut sockets = sockets();
        sockets.push(connection(
            Protocol::Tcp,
            address(9, 8080),
            State::Listen,
            4,
        ));
        assert_eq!(
            ordered(Column::LocalEndpoint, false, &sockets),
            [1, 4, 2, 3]
        );
    }

    #[test]
    fn pressing_a_heading_orders_the_table_by_it() {
        let mut connections = Connections {
            snapshot: Some(Snapshot {
                connections: sockets(),
                owners: owners(),
            }),
            ..Connections::default()
        };
        let processes = Processes::default();
        for message in pressed(connections.view(&processes), "Exposure") {
            drop(connections.update(message));
        }
        assert_eq!(connections.sorting.direction(Column::Exposure), Some(false));
    }
}
