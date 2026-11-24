use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Reads a one-value file such as a sysfs attribute.
pub fn read<T: FromStr>(path: impl AsRef<Path>) -> Option<T> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Entries of `root` named `prefix` followed by a number, such as `cpu0` or `hwmon3`.
pub fn numbered_dirs(
    root: impl AsRef<Path>,
    prefix: &'static str,
) -> impl Iterator<Item = PathBuf> {
    fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(move |entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|name| name.strip_prefix(prefix))
                .is_some_and(|id| id.parse::<usize>().is_ok())
        })
        .map(|entry| entry.path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_none() {
        assert_eq!(read::<u64>("/nonexistent/peepers"), None);
    }

    #[test]
    fn boot_id_parses_as_text() {
        assert!(read::<String>("/proc/sys/kernel/random/boot_id").is_some_and(|id| id.len() == 36));
    }

    #[test]
    fn numbered_dirs_ignore_other_names() {
        let found: Vec<_> = numbered_dirs("/sys/devices/system/cpu", "cpu").collect();
        assert_ne!(found.len(), 0);
        assert!(
            found
                .iter()
                .all(|path| path.file_name().is_some_and(|name| name != "cpufreq"))
        );
    }
}
