use std::collections::HashMap;
use std::fs;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Protocol {
    Tcp,
    Udp,
    UnixStream,
    UnixDatagram,
    UnixSeqPacket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Established,
    SynSent,
    SynReceived,
    FinWait1,
    FinWait2,
    TimeWait,
    Closed,
    CloseWait,
    LastAck,
    Listen,
    Closing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    Address(SocketAddr),
    Path(String),
    None,
}

/// How far a listening socket can be reached from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Exposure {
    Loopback,
    LocalNetwork,
    /// Bound to the unspecified address, so every interface answers.
    AllInterfaces,
    Public,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    pub protocol: Protocol,
    pub local: Endpoint,
    pub remote: Endpoint,
    pub state: State,
    /// Not recorded for UNIX sockets.
    pub uid: Option<u32>,
    pub inode: u64,
}

impl Connection {
    /// `None` for UNIX sockets, which no network can reach.
    #[must_use]
    pub fn exposure(&self) -> Option<Exposure> {
        match &self.local {
            Endpoint::Address(address) => Some(exposure(address.ip())),
            Endpoint::Path(_) | Endpoint::None => None,
        }
    }
}

fn exposure(address: IpAddr) -> Exposure {
    if address.is_unspecified() {
        Exposure::AllInterfaces
    } else if address.is_loopback() {
        Exposure::Loopback
    } else if local_network(address) {
        Exposure::LocalNetwork
    } else {
        Exposure::Public
    }
}

/// Private, link-local, unique-local and carrier-grade NAT ranges; the last is where overlay
/// networks such as Tailscale put their addresses.
fn local_network(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            address.is_private()
                || address.is_link_local()
                || (address.octets()[0] == 100 && address.octets()[1] & 0xc0 == 64)
        }
        IpAddr::V6(address) => {
            let first = address.segments()[0];
            first & 0xfe00 == 0xfc00 || first & 0xffc0 == 0xfe80
        }
    }
}

/// Every TCP, UDP and UNIX socket in this network namespace.
///
/// # Errors
///
/// Fails when the socket tables cannot be read.
pub fn read() -> io::Result<Vec<Connection>> {
    let mut connections = Vec::new();
    for (file, protocol) in [
        ("tcp", Protocol::Tcp),
        ("tcp6", Protocol::Tcp),
        ("udp", Protocol::Udp),
        ("udp6", Protocol::Udp),
    ] {
        let table = fs::read_to_string(format!("/proc/net/{file}"))?;
        connections.extend(
            table
                .lines()
                .skip(1)
                .filter_map(|line| parse_inet(protocol, line)),
        );
    }
    let unix = fs::read_to_string("/proc/net/unix")?;
    connections.extend(unix.lines().skip(1).filter_map(parse_unix));
    Ok(connections)
}

/// `sl local remote state tx:rx tr:when retransmit uid timeout inode ...`
fn parse_inet(protocol: Protocol, line: &str) -> Option<Connection> {
    let fields: Vec<&str> = line.split_ascii_whitespace().collect();
    let [_, local, remote, state, _, _, _, uid, _, inode, ..] = fields[..] else {
        return None;
    };
    Some(Connection {
        protocol,
        local: Endpoint::Address(address(local)?),
        remote: Endpoint::Address(address(remote)?),
        state: inet_state(u8::from_str_radix(state, 16).ok()?),
        uid: Some(uid.parse().ok()?),
        inode: inode.parse().ok()?,
    })
}

/// `num refcount protocol flags type state inode [path]`
fn parse_unix(line: &str) -> Option<Connection> {
    let mut rest = line;
    let [_, _, _, Some(flags), Some(kind), Some(state), Some(inode)] =
        [(); 7].map(|()| token(&mut rest))
    else {
        return None;
    };
    let listening = u32::from_str_radix(flags, 16).ok()? & ACCEPTING != 0;
    Some(Connection {
        protocol: match kind {
            "0001" => Protocol::UnixStream,
            "0002" => Protocol::UnixDatagram,
            _ => Protocol::UnixSeqPacket,
        },
        local: match rest.trim() {
            "" => Endpoint::None,
            path => Endpoint::Path(path.to_owned()),
        },
        remote: Endpoint::None,
        state: unix_state(u8::from_str_radix(state, 16).ok()?, listening),
        uid: None,
        inode: inode.parse().ok()?,
    })
}

/// `SO_ACCEPTCON`: set in the flags of a listening socket.
const ACCEPTING: u32 = 0x1_0000;

/// Splits the next whitespace-delimited token off `rest`; what remains keeps its own spacing.
fn token<'a>(rest: &mut &'a str) -> Option<&'a str> {
    let trimmed = rest.trim_start();
    let (token, tail) = trimmed
        .split_once(char::is_whitespace)
        .unwrap_or((trimmed, ""));
    *rest = tail;
    (!token.is_empty()).then_some(token)
}

/// `0100007F:0035`: the address as the kernel stores it, so little-endian words, then the port.
fn address(text: &str) -> Option<SocketAddr> {
    let (host, port) = text.split_once(':')?;
    let port = u16::from_str_radix(port, 16).ok()?;
    let ip = match host.len() {
        8 => IpAddr::V4(Ipv4Addr::from(
            u32::from_str_radix(host, 16).ok()?.to_le_bytes(),
        )),
        32 => {
            let mut bytes = [0_u8; 16];
            for (index, chunk) in bytes.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let word = u32::from_str_radix(host.get(index * 8..index * 8 + 8)?, 16).ok()?;
                *chunk = word.to_le_bytes();
            }
            IpAddr::V6(Ipv6Addr::from(bytes))
        }
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}

/// From `include/net/tcp_states.h`. UDP reuses the table: 07 is unconnected, 01 connected.
fn inet_state(code: u8) -> State {
    match code {
        0x01 => State::Established,
        0x02 => State::SynSent,
        0x03 => State::SynReceived,
        0x04 => State::FinWait1,
        0x05 => State::FinWait2,
        0x06 => State::TimeWait,
        0x08 => State::CloseWait,
        0x09 => State::LastAck,
        0x0a => State::Listen,
        0x0b => State::Closing,
        _ => State::Closed,
    }
}

/// `SS_UNCONNECTED`, `SS_CONNECTING`, `SS_CONNECTED`, `SS_DISCONNECTING`.
fn unix_state(code: u8, listening: bool) -> State {
    match (code, listening) {
        (_, true) => State::Listen,
        (2, false) => State::SynSent,
        (3, false) => State::Established,
        (4, false) => State::Closing,
        _ => State::Closed,
    }
}

/// The process holding each socket, found through the descriptors of every process this user
/// may inspect. Sockets of other users' processes stay unowned without privilege.
#[must_use]
pub fn owners() -> HashMap<u64, u32> {
    fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            Some((
                entry.file_name().to_str()?.parse::<u32>().ok()?,
                entry.path(),
            ))
        })
        .flat_map(|(pid, path)| {
            fs::read_dir(path.join("fd"))
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .filter_map(move |fd| Some((socket_inode(&fd.path())?, pid)))
        })
        .collect()
}

/// `socket:[12345]` links name a socket by inode.
fn socket_inode(descriptor: &Path) -> Option<u64> {
    let target = fs::read_link(descriptor).ok()?;
    target
        .to_str()?
        .strip_prefix("socket:[")?
        .strip_suffix(']')?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_addresses_are_little_endian_words() {
        assert_eq!(
            address("0100007F:0035"),
            Some("127.0.0.1:53".parse().unwrap())
        );
        assert_eq!(
            address("017AA8C0:C8D5"),
            Some("192.168.122.1:51413".parse().unwrap())
        );
    }

    #[test]
    fn ipv6_addresses_reverse_each_word() {
        let parsed = address("5C117AFD0000E0A100000000538D398F:01BB").unwrap();
        assert_eq!(parsed, "[fd7a:115c:a1e0::8f39:8d53]:443".parse().unwrap());
        assert_eq!(
            address("00000000000000000000000001000000:0050"),
            Some("[::1]:80".parse().unwrap())
        );
    }

    #[test]
    fn malformed_addresses_are_rejected() {
        assert_eq!(address("0100007F"), None);
        assert_eq!(address("0100007:0035"), None);
    }

    #[test]
    fn tcp_lines_carry_state_owner_uid_and_inode() {
        let line = "   1: 0100007F:DFFF 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 16577562 1 0 100 0 0 10 0";
        let connection = parse_inet(Protocol::Tcp, line).unwrap();
        assert_eq!(
            (connection.state, connection.uid, connection.inode),
            (State::Listen, Some(1000), 16_577_562)
        );
        assert_eq!(connection.exposure(), Some(Exposure::Loopback));
    }

    #[test]
    fn unix_lines_with_and_without_a_path() {
        let named = "0000000000000000: 00000002 00000000 00010000 0001 01 20347 /nix/var/nix/daemon-socket/socket";
        let connection = parse_unix(named).unwrap();
        assert_eq!(connection.state, State::Listen);
        assert_eq!(
            connection.local,
            Endpoint::Path("/nix/var/nix/daemon-socket/socket".to_owned())
        );
        let unnamed =
            parse_unix("0000000000000000: 00000003 00000000 00000000 0001 03 15338415").unwrap();
        assert_eq!(unnamed.state, State::Established);
        assert_eq!(unnamed.local, Endpoint::None);
        assert_eq!(unnamed.exposure(), None);
    }

    #[test]
    fn exposure_classifies_by_address() {
        let exposure = |text: &str| super::exposure(text.parse().unwrap());
        assert_eq!(exposure("0.0.0.0"), Exposure::AllInterfaces);
        assert_eq!(exposure("::"), Exposure::AllInterfaces);
        assert_eq!(exposure("127.0.0.1"), Exposure::Loopback);
        assert_eq!(exposure("192.168.1.4"), Exposure::LocalNetwork);
        assert_eq!(exposure("fd7a:115c::1"), Exposure::LocalNetwork);
        assert_eq!(exposure("fe80::1"), Exposure::LocalNetwork);
        assert_eq!(exposure("100.64.0.1"), Exposure::LocalNetwork);
        assert_eq!(exposure("100.128.0.1"), Exposure::Public);
        assert_eq!(exposure("8.8.8.8"), Exposure::Public);
    }

    #[test]
    fn the_live_tables_parse_without_losing_lines() {
        let connections = read().unwrap();
        let tcp_lines: usize = ["tcp", "tcp6"]
            .iter()
            .map(|file| {
                fs::read_to_string(format!("/proc/net/{file}"))
                    .unwrap()
                    .lines()
                    .count()
                    - 1
            })
            .sum();
        assert!(
            connections
                .iter()
                .filter(|c| c.protocol == Protocol::Tcp)
                .count()
                .abs_diff(tcp_lines)
                <= 2
        );
    }

    #[test]
    fn this_process_owns_its_own_sockets() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let inode = read()
            .unwrap()
            .into_iter()
            .find(|c| c.local == Endpoint::Address(format!("127.0.0.1:{port}").parse().unwrap()))
            .unwrap()
            .inode;
        assert_eq!(owners().get(&inode), Some(&std::process::id()));
    }
}
