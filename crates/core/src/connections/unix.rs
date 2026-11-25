//! UNIX sockets from the kernel's socket diagnostics, the interface `ss` uses. Names arrive as
//! counted bytes, so a name holding a newline cannot pass for another socket the way it can in the
//! text of `/proc/net/unix`.

use std::io;

use rustix::net::{AddressFamily, RecvFlags, SendFlags, SocketType, netlink, recv, sendto, socket};

use super::{Connection, Endpoint, Protocol, State};

/// From the kernel's `socket.h`, `netlink.h`, `sock_diag.h` and `unix_diag.h`.
const AF_UNIX: u8 = 1;
const SOCK_DIAG_BY_FAMILY: u16 = 20;
const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;
const NLM_F_REQUEST: u16 = 0x1;
const NLM_F_DUMP: u16 = 0x300;
const UDIAG_SHOW_NAME: u32 = 0x1;
const UNIX_DIAG_NAME: u16 = 0;
/// `nlmsghdr`, and `unix_diag_msg` after it.
const HEADER: usize = 16;
const MESSAGE: usize = 16;
/// `nlmsghdr` and `unix_diag_req`.
const REQUEST: u32 = 40;
/// `TCP_ESTABLISHED` and `TCP_LISTEN`, which UNIX sockets borrow for their state.
const ESTABLISHED: u8 = 1;
const LISTENING: u8 = 10;

/// Every UNIX socket in this network namespace.
pub(super) fn read() -> io::Result<Vec<Connection>> {
    let diag = socket(
        AddressFamily::NETLINK,
        SocketType::RAW,
        Some(netlink::SOCK_DIAG),
    )?;
    sendto(
        &diag,
        &request(),
        SendFlags::empty(),
        &netlink::SocketAddrNetlink::new(0, 0),
    )?;
    let mut sockets = Vec::new();
    let mut buffer = vec![0; 64 << 10];
    loop {
        let (received, whole) = recv(&diag, &mut buffer[..], RecvFlags::empty())?;
        if whole > received {
            return Err(io::ErrorKind::InvalidData.into());
        }
        if parse(&buffer[..received], &mut sockets)? {
            return Ok(sockets);
        }
    }
}

/// `nlmsghdr`, then a `unix_diag_req` for every state and any inode, with names and no cookie.
fn request() -> Vec<u8> {
    let mut request = Vec::with_capacity(REQUEST as usize);
    request.extend(REQUEST.to_ne_bytes());
    request.extend(SOCK_DIAG_BY_FAMILY.to_ne_bytes());
    request.extend((NLM_F_REQUEST | NLM_F_DUMP).to_ne_bytes());
    request.extend([0; 8]);
    request.extend([AF_UNIX, 0, 0, 0]);
    request.extend(u32::MAX.to_ne_bytes());
    request.extend(0_u32.to_ne_bytes());
    request.extend(UDIAG_SHOW_NAME.to_ne_bytes());
    request.extend(u64::MAX.to_ne_bytes());
    request
}

/// Adds the sockets in one batch of replies, and says whether the dump has ended.
fn parse(mut batch: &[u8], sockets: &mut Vec<Connection>) -> io::Result<bool> {
    while let Some(header) = batch.first_chunk::<HEADER>() {
        let length = word(header, 0).ok_or(io::ErrorKind::InvalidData)? as usize;
        let message = batch
            .get(HEADER..length)
            .ok_or(io::ErrorKind::InvalidData)?;
        match u16::from_ne_bytes([header[4], header[5]]) {
            NLMSG_DONE => return Ok(true),
            NLMSG_ERROR => {
                let code = word(message, 0).ok_or(io::ErrorKind::InvalidData)?;
                return Err(io::Error::from_raw_os_error(
                    i32::from_ne_bytes(code.to_ne_bytes()).wrapping_neg(),
                ));
            }
            SOCK_DIAG_BY_FAMILY => sockets.extend(connection(message)),
            _ => {}
        }
        batch = batch.get(aligned(length)..).unwrap_or_default();
    }
    Ok(false)
}

/// `family type state pad inode cookie[2]`, then attributes.
fn connection(message: &[u8]) -> Option<Connection> {
    let &[_, kind, state, _] = message.first_chunk::<4>()?;
    let inode = word(message, 4)?;
    let mut name = None;
    let mut attributes = message.get(MESSAGE..)?;
    while let Some(&[low, high, kind_low, kind_high]) = attributes.first_chunk::<4>() {
        let length = usize::from(u16::from_ne_bytes([low, high]));
        let value = attributes.get(4..length)?;
        if u16::from_ne_bytes([kind_low, kind_high]) == UNIX_DIAG_NAME {
            name = Some(value);
        }
        attributes = attributes.get(aligned(length)..).unwrap_or_default();
    }
    Some(Connection {
        protocol: match kind {
            1 => Protocol::UnixStream,
            2 => Protocol::UnixDatagram,
            _ => Protocol::UnixSeqPacket,
        },
        local: name.map_or(Endpoint::None, endpoint),
        remote: Endpoint::None,
        state: match state {
            LISTENING => State::Listen,
            ESTABLISHED => State::Established,
            _ => State::Closed,
        },
        uid: None,
        inode: inode.into(),
    })
}

/// An abstract name starts with a NUL and is shown with `@`, as `ss` shows it; a path ends in one.
fn endpoint(name: &[u8]) -> Endpoint {
    match name {
        [] => Endpoint::None,
        [0, rest @ ..] => {
            Endpoint::Path(format!("@{}", String::from_utf8_lossy(rest)).replace('\0', "@"))
        }
        path => Endpoint::Path(
            String::from_utf8_lossy(path.split(|&byte| byte == 0).next().unwrap_or_default())
                .into_owned(),
        ),
    }
}

fn word(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_ne_bytes(*bytes.get(at..)?.first_chunk()?))
}

/// Netlink messages and attributes are padded to four bytes.
fn aligned(length: usize) -> usize {
    length.next_multiple_of(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(kind: u16, body: &[u8]) -> Vec<u8> {
        let mut message = Vec::new();
        message.extend(u32::try_from(HEADER + body.len()).unwrap().to_ne_bytes());
        message.extend(kind.to_ne_bytes());
        message.extend([0; 10]);
        message.extend(body);
        message.resize(aligned(message.len()), 0);
        message
    }

    fn socket(kind: u8, state: u8, inode: u32, name: Option<&[u8]>) -> Vec<u8> {
        let mut body = vec![1, kind, state, 0];
        body.extend(inode.to_ne_bytes());
        body.extend([0; 8]);
        if let Some(name) = name {
            body.extend(u16::try_from(4 + name.len()).unwrap().to_ne_bytes());
            body.extend(UNIX_DIAG_NAME.to_ne_bytes());
            body.extend(name);
            body.resize(aligned(body.len()), 0);
        }
        reply(SOCK_DIAG_BY_FAMILY, &body)
    }

    #[test]
    fn replies_give_each_socket_its_kind_state_and_name() {
        let mut batch = socket(
            1,
            LISTENING,
            20347,
            Some(b"/nix/var/nix/daemon-socket/socket\0"),
        );
        batch.extend(socket(2, 7, 31, Some(b"\0peepers\0x")));
        batch.extend(socket(5, ESTABLISHED, 15_338_415, None));
        let mut sockets = Vec::new();
        assert!(!parse(&batch, &mut sockets).unwrap());
        let summary: Vec<_> = sockets
            .iter()
            .map(|c| (c.protocol, c.state, c.inode, c.local.clone()))
            .collect();
        assert_eq!(
            summary,
            [
                (
                    Protocol::UnixStream,
                    State::Listen,
                    20347,
                    Endpoint::Path("/nix/var/nix/daemon-socket/socket".to_owned())
                ),
                (
                    Protocol::UnixDatagram,
                    State::Closed,
                    31,
                    Endpoint::Path("@peepers@x".to_owned())
                ),
                (
                    Protocol::UnixSeqPacket,
                    State::Established,
                    15_338_415,
                    Endpoint::None
                ),
            ]
        );
    }

    #[test]
    fn the_dump_ends_at_done_and_an_error_is_returned() {
        let mut sockets = Vec::new();
        assert!(parse(&reply(NLMSG_DONE, &[0; 4]), &mut sockets).unwrap());
        let error = parse(&reply(NLMSG_ERROR, &(-13_i32).to_ne_bytes()), &mut sockets).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(13));
    }

    #[test]
    fn a_message_longer_than_its_batch_is_refused() {
        let mut batch = socket(1, LISTENING, 1, None);
        batch.truncate(HEADER + 4);
        assert_eq!(
            parse(&batch, &mut Vec::new()).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn the_kernel_lists_a_socket_this_process_just_bound() {
        use std::os::linux::net::SocketAddrExt;
        use std::os::unix::net::{SocketAddr, UnixListener};
        let _listener =
            UnixListener::bind_addr(&SocketAddr::from_abstract_name(b"peepers-diag").unwrap())
                .unwrap();
        assert!(
            read()
                .unwrap()
                .iter()
                .any(|c| c.local == Endpoint::Path("@peepers-diag".to_owned())
                    && c.state == State::Listen)
        );
    }
}
