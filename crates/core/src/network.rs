use std::collections::HashMap;
use std::fs;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, Instant};

use nix::ifaddrs::getifaddrs;

use crate::math::per_second;
use crate::pci;
use crate::sysfs;

const NET_DIR: &str = "/sys/class/net";
/// `ARPHRD_LOOPBACK` from `linux/if_arp.h`.
const LOOPBACK_TYPE: u32 = 772;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Loopback,
    Ethernet,
    Wireless,
    Virtual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Signal {
    pub quality: u32,
    pub dbm: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interface {
    pub name: String,
    pub model: Option<String>,
    pub kind: Kind,
    pub state: Option<String>,
    pub mac: Option<String>,
    pub speed_mbps: Option<u32>,
    pub ipv4: Vec<Ipv4Addr>,
    pub ipv6: Vec<Ipv6Addr>,
    pub signal: Option<Signal>,
    pub received: u64,
    pub sent: u64,
    pub receive_per_second: u64,
    pub send_per_second: u64,
}

#[derive(Debug, Default)]
pub struct Sampler {
    before: HashMap<String, (u64, u64)>,
    at: Option<Instant>,
}

impl Sampler {
    /// # Errors
    ///
    /// Fails when `/proc/net/dev` cannot be read.
    pub fn sample(&mut self) -> io::Result<Vec<Interface>> {
        let now = Instant::now();
        let elapsed = self.at.replace(now).map_or(Duration::ZERO, |at| now - at);
        let counters = parse_dev(&fs::read_to_string("/proc/net/dev")?);
        let signals = parse_wireless(&fs::read_to_string("/proc/net/wireless").unwrap_or_default());
        let mut addresses = addresses();

        let mut interfaces: Vec<_> = counters
            .iter()
            .map(|(name, &(received, sent))| {
                let (earlier_received, earlier_sent) =
                    self.before.get(name).copied().unwrap_or((received, sent));
                let device = Path::new(NET_DIR).join(name);
                let (ipv4, ipv6) = addresses.remove(name).unwrap_or_default();
                Interface {
                    name: name.clone(),
                    model: pci::model(&device.join("device")),
                    kind: kind(&device),
                    state: sysfs::read(device.join("operstate")),
                    mac: sysfs::read(device.join("address")),
                    speed_mbps: sysfs::read::<i32>(device.join("speed"))
                        .and_then(|speed| u32::try_from(speed).ok()),
                    ipv4,
                    ipv6,
                    signal: signals.get(name).copied(),
                    received,
                    sent,
                    receive_per_second: per_second(
                        received.saturating_sub(earlier_received),
                        elapsed,
                    ),
                    send_per_second: per_second(sent.saturating_sub(earlier_sent), elapsed),
                }
            })
            .collect();
        interfaces.sort_by(|a, b| a.name.cmp(&b.name));

        self.before = counters;
        Ok(interfaces)
    }
}

fn kind(device: &Path) -> Kind {
    if sysfs::read(device.join("type")) == Some(LOOPBACK_TYPE) {
        Kind::Loopback
    } else if device.join("wireless").exists() {
        Kind::Wireless
    } else if fs::canonicalize(device).is_ok_and(|real| real.starts_with("/sys/devices/virtual")) {
        Kind::Virtual
    } else {
        Kind::Ethernet
    }
}

fn addresses() -> HashMap<String, (Vec<Ipv4Addr>, Vec<Ipv6Addr>)> {
    let mut found: HashMap<String, (Vec<_>, Vec<_>)> = HashMap::new();
    for entry in getifaddrs().into_iter().flatten() {
        let Some(address) = entry.address else {
            continue;
        };
        let (ipv4, ipv6) = found.entry(entry.interface_name).or_default();
        if let Some(address) = address.as_sockaddr_in() {
            ipv4.push(address.ip());
        } else if let Some(address) = address.as_sockaddr_in6() {
            ipv6.push(address.ip());
        }
    }
    found
}

/// Received then transmitted bytes per interface; the first eight columns are the receive side.
fn parse_dev(text: &str) -> HashMap<String, (u64, u64)> {
    text.lines()
        .skip(2)
        .filter_map(|line| {
            let (name, columns) = line.split_once(':')?;
            let values = columns
                .split_ascii_whitespace()
                .map(|c| c.parse().ok())
                .collect::<Option<Vec<u64>>>()?;
            let [received, _, _, _, _, _, _, _, sent, ..] = values[..] else {
                return None;
            };
            Some((name.trim().to_owned(), (received, sent)))
        })
        .collect()
}

/// Link quality and signal level; the kernel prints both with a trailing dot.
fn parse_wireless(text: &str) -> HashMap<String, Signal> {
    text.lines()
        .skip(2)
        .filter_map(|line| {
            let (name, columns) = line.split_once(':')?;
            let [_, quality, level, ..] = columns.split_ascii_whitespace().collect::<Vec<_>>()[..]
            else {
                return None;
            };
            Some((
                name.trim().to_owned(),
                Signal {
                    quality: number(quality)?,
                    dbm: number(level)?,
                },
            ))
        })
        .collect()
}

fn number<T: FromStr>(field: &str) -> Option<T> {
    field.trim_end_matches('.').parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEV: &str = "Inter-|   Receive |  Transmit\n face |bytes packets errs drop fifo frame compressed multicast|bytes packets errs drop fifo colls carrier compressed\n\
                       lo: 100 1 0 0 0 0 0 0 100 1 0 0 0 0 0 0\n\
                       enp5s0: 5000 10 0 0 0 0 0 0 900 8 0 0 0 0 0 0\n";
    const WIRELESS: &str = "Inter-| sta-|   Quality        |   Discarded packets\n face | tus | link level noise |  nwid\n\
                            wlp6s0: 0000   68.  -42.  -256        0\n";

    #[test]
    fn dev_counters_pair_received_with_sent_bytes() {
        let counters = parse_dev(DEV);
        assert_eq!(counters["enp5s0"], (5000, 900));
        assert_eq!(counters["lo"], (100, 100));
    }

    #[test]
    fn wireless_signal_drops_the_trailing_dots() {
        assert_eq!(
            parse_wireless(WIRELESS)["wlp6s0"],
            Signal {
                quality: 68,
                dbm: -42
            }
        );
    }

    #[test]
    fn loopback_is_recognised_by_its_hardware_type() {
        assert_eq!(kind(Path::new("/sys/class/net/lo")), Kind::Loopback);
    }

    #[test]
    fn sampling_lists_the_loopback_with_its_address() {
        let interfaces = Sampler::default().sample().unwrap();
        let loopback = interfaces
            .iter()
            .find(|interface| interface.name == "lo")
            .unwrap();
        assert!(loopback.ipv4.contains(&Ipv4Addr::LOCALHOST));
    }
}
