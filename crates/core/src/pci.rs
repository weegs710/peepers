use std::fs;
use std::path::Path;

const UDEV_DATABASE: &str = "/run/udev/data";

/// The database name of the PCI device at or above a `device` link, such as
/// `Navi 23 [Radeon RX 6600/6600 XT/6600M]`. Udev resolves it from the PCI ID database.
#[must_use]
pub fn model(device: &Path) -> Option<String> {
    fs::canonicalize(device)
        .ok()?
        .ancestors()
        .find_map(|directory| {
            let slot = directory.file_name()?.to_str()?;
            parse_model(&fs::read_to_string(format!("{UDEV_DATABASE}/+pci:{slot}")).ok()?)
        })
}

fn parse_model(entry: &str) -> Option<String> {
    entry
        .lines()
        .find_map(|line| line.strip_prefix("E:ID_MODEL_FROM_DATABASE="))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_model_is_the_database_property() {
        let entry = "S:dri/card1\nE:ID_VENDOR_FROM_DATABASE=AMD\nE:ID_MODEL_FROM_DATABASE=Navi 23 [Radeon RX 6600M]\n";
        assert_eq!(
            parse_model(entry).as_deref(),
            Some("Navi 23 [Radeon RX 6600M]")
        );
    }

    #[test]
    fn entries_without_a_model_have_none() {
        assert_eq!(parse_model("E:ID_VENDOR_FROM_DATABASE=AMD\n"), None);
    }

    #[test]
    fn a_device_without_a_link_has_no_model() {
        assert_eq!(model(Path::new("/sys/class/net/lo/device")), None);
    }
}
