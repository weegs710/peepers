mod fdinfo;
mod nvidia;

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use crate::pci;
use crate::sensors::{self, Reading};
use crate::sysfs;

const DRM_DIR: &str = "/sys/class/drm";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vendor {
    Amd,
    Nvidia,
    Intel,
    Other(u16),
}

/// Adds driver-specific counters to the DRM inventory. Intel engine times need two samples.
#[derive(Debug, Default)]
pub struct Sampler {
    intel: fdinfo::Sampler,
    nvidia: sensors::Cached<Vec<nvidia::Metrics>>,
}

impl Sampler {
    pub fn sample(&mut self) -> Vec<Adapter> {
        let mut adapters = Adapter::read_all();
        let intel = if adapters
            .iter()
            .any(|gpu| gpu.vendor == Vendor::Intel && gpu.busy.is_none())
        {
            self.intel.sample()
        } else {
            HashMap::new()
        };
        let nvidia = if adapters.iter().any(|gpu| gpu.vendor == Vendor::Nvidia) {
            self.nvidia.get(nvidia::read)
        } else {
            Vec::new()
        };
        for adapter in &mut adapters {
            let Some(address) =
                fs::canonicalize(Path::new(DRM_DIR).join(&adapter.card).join("device"))
                    .ok()
                    .and_then(|path| pci_address(path.file_name()?.to_str()?))
            else {
                continue;
            };
            match adapter.vendor {
                Vendor::Intel => {
                    adapter.busy = adapter.busy.or_else(|| intel.get(&address).copied());
                }
                Vendor::Nvidia => {
                    if let Some(metrics) = nvidia.iter().find(|metrics| metrics.address == address)
                    {
                        adapter.busy = metrics.busy.or(adapter.busy);
                        adapter.vram = metrics.vram.or(adapter.vram);
                    }
                }
                _ => {}
            }
        }
        adapters
    }
}

/// NVIDIA prints eight domain digits; sysfs and fdinfo normally print four.
fn pci_address(text: &str) -> Option<String> {
    let parts = text
        .trim()
        .split([':', '.'])
        .map(|part| u32::from_str_radix(part, 16).ok())
        .collect::<Option<Vec<_>>>()?;
    let [domain, bus, device, function] = parts.as_slice() else {
        return None;
    };
    if *bus > 255 || *device > 31 || *function > 7 {
        return None;
    }
    Some(format!("{domain:04x}:{bus:02x}:{device:02x}.{function:x}"))
}

impl Vendor {
    /// The PCI vendor id.
    #[must_use]
    pub fn id(self) -> u16 {
        match self {
            Self::Amd => 0x1002,
            Self::Nvidia => 0x10de,
            Self::Intel => 0x8086,
            Self::Other(id) => id,
        }
    }
}

impl From<u16> for Vendor {
    fn from(id: u16) -> Self {
        match id {
            0x1002 => Self::Amd,
            0x10de => Self::Nvidia,
            0x8086 => Self::Intel,
            other => Self::Other(other),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pool {
    pub total: u64,
    pub used: u64,
}

/// One DRM card. Fields the driver does not expose are `None` or empty.
#[derive(Debug, Clone, PartialEq)]
pub struct Adapter {
    pub card: String,
    pub vendor: Vendor,
    pub device: u16,
    pub model: Option<String>,
    pub driver: Option<String>,
    pub primary: bool,
    pub busy: Option<f32>,
    pub vram: Option<Pool>,
    pub gtt: Option<Pool>,
    pub link_speed: Option<String>,
    pub link_width: Option<u8>,
    pub sensors: Vec<Reading>,
}

impl Adapter {
    #[must_use]
    pub fn read_all() -> Vec<Self> {
        let mut adapters: Vec<_> = fs::read_dir(DRM_DIR)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter(|entry| is_card(&entry.file_name()))
            .filter_map(|entry| Self::read(&entry.path()))
            .collect();
        adapters.sort_by(|a, b| a.card.cmp(&b.card));
        adapters
    }

    fn read(card: &Path) -> Option<Self> {
        let device = card.join("device");
        Some(Self {
            card: card.file_name()?.to_str()?.to_owned(),
            vendor: hex(&device.join("vendor"))?.into(),
            device: hex(&device.join("device"))?,
            model: pci::model(&device),
            driver: fs::read_link(device.join("driver"))
                .ok()
                .and_then(|driver| Some(driver.file_name()?.to_str()?.to_owned())),
            primary: sysfs::read::<u8>(device.join("boot_vga")) == Some(1),
            busy: sysfs::read(device.join("gpu_busy_percent")),
            vram: pool(&device, "vram"),
            gtt: pool(&device, "gtt"),
            link_speed: sysfs::read(device.join("current_link_speed")),
            link_width: sysfs::read(device.join("current_link_width")),
            sensors: sensors::read_device(&device),
        })
    }
}

/// `card0` qualifies; its connectors such as `card0-DP-1` do not.
fn is_card(name: &OsStr) -> bool {
    name.to_str()
        .and_then(|name| name.strip_prefix("card"))
        .is_some_and(|id| id.parse::<u32>().is_ok())
}

fn hex(path: &Path) -> Option<u16> {
    let text: String = sysfs::read(path)?;
    u16::from_str_radix(text.strip_prefix("0x")?, 16).ok()
}

fn pool(device: &Path, name: &str) -> Option<Pool> {
    Some(Pool {
        total: sysfs::read(device.join(format!("mem_info_{name}_total")))?,
        used: sysfs::read(device.join(format!("mem_info_{name}_used")))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_pci_vendors_get_names() {
        assert_eq!(Vendor::from(0x1002), Vendor::Amd);
        assert_eq!(Vendor::from(0x10de), Vendor::Nvidia);
        assert_eq!(Vendor::from(0x8086), Vendor::Intel);
        assert_eq!(Vendor::from(0x1234), Vendor::Other(0x1234));
    }

    #[test]
    fn connectors_are_not_cards() {
        assert!(is_card(OsStr::new("card0")));
        assert!(is_card(OsStr::new("card12")));
        assert!(!is_card(OsStr::new("card0-DP-1")));
        assert!(!is_card(OsStr::new("renderD128")));
    }

    #[test]
    fn adapters_are_ordered_by_card() {
        let adapters = Adapter::read_all();
        assert!(adapters.windows(2).all(|pair| pair[0].card <= pair[1].card));
    }
}
