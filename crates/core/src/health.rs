use std::collections::HashMap;

use zbus::blocking::proxy::Builder;
use zbus::blocking::{Connection, Proxy};
use zbus::fdo;
use zbus::proxy::CacheProperties;
use zvariant::{OwnedObjectPath, OwnedValue, Value};

use crate::disk::Kind;

const DESTINATION: &str = "org.freedesktop.UDisks2";
const BLOCK_DEVICES: &str = "/org/freedesktop/UDisks2/block_devices";
const BLOCK: &str = "org.freedesktop.UDisks2.Block";
const CONTROLLER: &str = "org.freedesktop.UDisks2.NVMe.Controller";
const ATA: &str = "org.freedesktop.UDisks2.Drive.Ata";

/// Rated wear past which a drive is worth replacing before it fails.
const WORN_PERCENT: u8 = 90;

/// A drive's own account of its condition, as `udisks2` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Health {
    Nvme(Nvme),
    Ata(Ata),
}

impl Health {
    /// What is wrong, in words; empty when the drive reports nothing to worry about.
    #[must_use]
    pub fn concerns(&self) -> Vec<String> {
        match self {
            Self::Nvme(nvme) => nvme.concerns(),
            Self::Ata(ata) => ata.concerns(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nvme {
    /// Share of the rated endurance used up; may pass 100.
    pub percent_used: u8,
    pub spare: u8,
    pub spare_threshold: u8,
    /// Estimated from the drive's own counters; `None` while it reports zero.
    pub bytes_read: Option<u64>,
    pub bytes_written: Option<u64>,
    pub power_on_hours: u64,
    pub power_cycles: u64,
    pub unsafe_shutdowns: u64,
    pub media_errors: u64,
    /// What the drive raises on its own account, such as `spare` or `readonly`.
    pub warnings: Vec<String>,
}

impl Nvme {
    fn concerns(&self) -> Vec<String> {
        let mut concerns = Vec::new();
        if !self.warnings.is_empty() {
            concerns.push(format!("The drive warns of {}", self.warnings.join(", ")));
        }
        if self.media_errors > 0 {
            concerns.push(format!("{} media errors", self.media_errors));
        }
        if self.percent_used >= WORN_PERCENT {
            concerns.push(format!(
                "{}% of its rated endurance is used",
                self.percent_used
            ));
        }
        concerns
    }
}

/// A SATA or other `ATA` drive's SMART summary. A count is `None` where the drive does not say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ata {
    /// The drive's own verdict that it is about to fail.
    pub failing: bool,
    pub attributes_failing: Option<u32>,
    pub attributes_failed_before: Option<u32>,
    /// Reallocated and pending sectors together.
    pub bad_sectors: Option<u64>,
    pub power_on_hours: Option<u64>,
    /// The state of the last self-test, such as `success` or `error_read`.
    pub self_test: String,
}

impl Ata {
    fn concerns(&self) -> Vec<String> {
        let mut concerns = Vec::new();
        if self.failing {
            concerns.push("The drive says it is about to fail".to_owned());
        }
        if let Some(count @ 1..) = self.attributes_failing {
            concerns.push(format!("{count} SMART attributes failing"));
        }
        if let Some(count @ 1..) = self.bad_sectors {
            concerns.push(format!("{count} bad sectors"));
        }
        if self.self_test.starts_with("error") || self.self_test == "fatal" {
            concerns.push(format!("The last self-test failed ({})", self.self_test));
        }
        concerns
    }
}

/// Reads the health of the drive behind a block device such as `nvme0n1` or `sda`. Everything
/// comes from `udisks2` without asking anyone to authenticate: a refresh of a SATA drive's SMART
/// data is attempted, and when it is refused or the disk is asleep the reading it already holds is
/// used.
///
/// # Errors
///
/// Fails when `udisks2` is not running, does not know the device, or has no SMART data for it.
pub fn read(device: &str, kind: Kind) -> Result<Health, String> {
    let connection = Connection::system().map_err(|error| error.to_string())?;
    let block = proxy(&connection, format!("{BLOCK_DEVICES}/{device}"), BLOCK)?;
    let drive: OwnedObjectPath = block.get_property("Drive").map_err(explain)?;
    match kind {
        Kind::Nvme => nvme(&proxy(&connection, drive.to_string(), CONTROLLER)?).map(Health::Nvme),
        Kind::Ssd | Kind::Hdd => ata(&proxy(&connection, drive.to_string(), ATA)?).map(Health::Ata),
    }
}

fn nvme(controller: &Proxy) -> Result<Nvme, String> {
    let attributes: HashMap<String, OwnedValue> = controller
        .call("SmartGetAttributes", &HashMap::<&str, Value>::new())
        .map_err(explain)?;
    let count = |key: &str| number::<u64>(&attributes, key);
    Ok(Nvme {
        percent_used: number(&attributes, "percent_used").ok_or("no wear figure")?,
        spare: number(&attributes, "avail_spare").ok_or("no spare figure")?,
        spare_threshold: number(&attributes, "spare_thresh").ok_or("no spare figure")?,
        bytes_read: count("total_data_read"),
        bytes_written: count("total_data_written"),
        power_on_hours: controller
            .get_property("SmartPowerOnHours")
            .map_err(explain)?,
        power_cycles: count("power_cycles").unwrap_or(0),
        unsafe_shutdowns: count("unsafe_shutdowns").unwrap_or(0),
        media_errors: count("media_errors").unwrap_or(0),
        warnings: controller
            .get_property("SmartCriticalWarning")
            .map_err(explain)?,
    })
}

fn ata(drive: &Proxy) -> Result<Ata, String> {
    let supported: bool = drive.get_property("SmartSupported").map_err(explain)?;
    if !supported {
        return Err("the drive does not support SMART".to_owned());
    }
    let enabled: bool = drive.get_property("SmartEnabled").map_err(explain)?;
    if !enabled {
        return Err("SMART is switched off on the drive".to_owned());
    }
    // Refused or asleep, the reading udisks already holds is still worth showing.
    drop(drive.call::<_, _, ()>(
        "SmartUpdate",
        &HashMap::from([("nowakeup", Value::from(true))]),
    ));
    let updated: u64 = drive.get_property("SmartUpdated").map_err(explain)?;
    if updated == 0 {
        return Err("udisks has not read this drive's SMART data yet".to_owned());
    }
    let known = |value: i64| u64::try_from(value).ok();
    let seconds: u64 = drive.get_property("SmartPowerOnSeconds").map_err(explain)?;
    let failing: i32 = drive
        .get_property("SmartNumAttributesFailing")
        .map_err(explain)?;
    let failed_before: i32 = drive
        .get_property("SmartNumAttributesFailedInThePast")
        .map_err(explain)?;
    let bad_sectors: i64 = drive.get_property("SmartNumBadSectors").map_err(explain)?;
    Ok(Ata {
        failing: drive.get_property("SmartFailing").map_err(explain)?,
        attributes_failing: u32::try_from(failing).ok(),
        attributes_failed_before: u32::try_from(failed_before).ok(),
        bad_sectors: known(bad_sectors),
        power_on_hours: (seconds > 0).then_some(seconds / 3600),
        self_test: drive.get_property("SmartSelftestStatus").map_err(explain)?,
    })
}

fn proxy(
    connection: &Connection,
    path: String,
    interface: &'static str,
) -> Result<Proxy<'static>, String> {
    Builder::new(connection)
        .destination(DESTINATION)
        .and_then(|builder| builder.path(path))
        .and_then(|builder| builder.interface(interface))
        .map(|builder| builder.cache_properties(CacheProperties::No))
        .and_then(Builder::build)
        .map_err(explain)
}

/// A drive that `udisks2` has no SMART interface for says so plainly; the daemon reports a missing
/// interface as `InvalidArgs`.
fn explain(error: zbus::Error) -> String {
    match error {
        zbus::Error::FDO(inner)
            if matches!(
                *inner,
                fdo::Error::InvalidArgs(_)
                    | fdo::Error::UnknownInterface(_)
                    | fdo::Error::UnknownProperty(_)
                    | fdo::Error::UnknownMethod(_)
            ) =>
        {
            "udisks reports no SMART data for this drive".to_owned()
        }
        other => other.to_string(),
    }
}

fn number<T: TryFrom<OwnedValue>>(
    attributes: &HashMap<String, OwnedValue>,
    key: &str,
) -> Option<T> {
    T::try_from(attributes.get(key)?.try_clone().ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy() -> Nvme {
        Nvme {
            percent_used: 2,
            spare: 100,
            spare_threshold: 10,
            bytes_read: Some(13_935_985_664_000),
            bytes_written: Some(27_993_332_736_000),
            power_on_hours: 425,
            power_cycles: 623,
            unsafe_shutdowns: 71,
            media_errors: 0,
            warnings: Vec::new(),
        }
    }

    fn sound() -> Ata {
        Ata {
            failing: false,
            attributes_failing: Some(0),
            attributes_failed_before: Some(0),
            bad_sectors: Some(0),
            power_on_hours: Some(12_000),
            self_test: "success".to_owned(),
        }
    }

    #[test]
    fn a_drive_with_nothing_to_report_has_no_concerns() {
        assert!(Health::Nvme(healthy()).concerns().is_empty());
        assert!(Health::Ata(sound()).concerns().is_empty());
    }

    #[test]
    fn every_kind_of_nvme_trouble_is_named() {
        let worn = Nvme {
            percent_used: 95,
            media_errors: 3,
            warnings: vec!["spare".to_owned(), "readonly".to_owned()],
            ..healthy()
        };
        assert_eq!(
            Health::Nvme(worn).concerns(),
            [
                "The drive warns of spare, readonly",
                "3 media errors",
                "95% of its rated endurance is used",
            ]
        );
    }

    #[test]
    fn wear_just_under_the_limit_is_not_a_concern() {
        let almost = Nvme {
            percent_used: WORN_PERCENT - 1,
            ..healthy()
        };
        assert!(Health::Nvme(almost).concerns().is_empty());
    }

    #[test]
    fn every_kind_of_ata_trouble_is_named() {
        let dying = Ata {
            failing: true,
            attributes_failing: Some(2),
            bad_sectors: Some(8),
            self_test: "error_read".to_owned(),
            ..sound()
        };
        assert_eq!(
            Health::Ata(dying).concerns(),
            [
                "The drive says it is about to fail",
                "2 SMART attributes failing",
                "8 bad sectors",
                "The last self-test failed (error_read)",
            ]
        );
    }

    #[test]
    fn counts_the_drive_does_not_give_are_not_concerns() {
        let silent = Ata {
            attributes_failing: None,
            attributes_failed_before: None,
            bad_sectors: None,
            power_on_hours: None,
            self_test: String::new(),
            ..sound()
        };
        assert!(Health::Ata(silent).concerns().is_empty());
    }

    fn nvme_name() -> Option<String> {
        std::fs::read_dir("/sys/block")
            .unwrap()
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .find(|name| name.starts_with("nvme"))
    }

    #[test]
    fn a_drive_on_this_machine_reports_its_health() {
        let Some(name) = nvme_name() else {
            return;
        };
        let Health::Nvme(health) = read(&name, Kind::Nvme).unwrap() else {
            panic!("an NVMe drive reports NVMe health");
        };
        assert!(health.spare <= 100);
        assert!(health.power_on_hours > 0);
    }

    #[test]
    fn a_drive_without_sata_smart_says_so_plainly() {
        let Some(name) = nvme_name() else {
            return;
        };
        assert_eq!(
            read(&name, Kind::Ssd).unwrap_err(),
            "udisks reports no SMART data for this drive"
        );
    }
}
