use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::sysfs;

const SYS_MODULE: &str = "/sys/module";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Live,
    Loading,
    Unloading,
}

/// A kernel module, loaded or compiled into the kernel image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Driver {
    pub name: String,
    /// `None` for built-in modules, which have no size of their own.
    pub size: Option<u64>,
    pub state: Option<State>,
    pub used_by: Vec<String>,
    pub source_version: Option<String>,
    pub taint: Option<String>,
    pub parameters: usize,
    pub path: Option<PathBuf>,
    pub depends: Vec<String>,
}

impl Driver {
    #[must_use]
    pub fn built_in(&self) -> bool {
        self.state.is_none()
    }
}

/// Loaded modules first-class, then the modules built into the kernel image, sorted by name.
#[must_use]
pub fn read() -> Vec<Driver> {
    let catalog = fs::read_to_string("/proc/sys/kernel/osrelease")
        .ok()
        .and_then(|release| catalog(release.trim()))
        .unwrap_or_default();
    let loaded: Vec<LoadedLine> = fs::read_to_string("/proc/modules")
        .unwrap_or_default()
        .lines()
        .filter_map(parse_loaded)
        .collect();

    let mut drivers: Vec<Driver> = loaded
        .iter()
        .map(|line| driver(&line.name, Some(line), &catalog))
        .collect();
    drivers.extend(
        fs::read_dir(SYS_MODULE)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter(|entry| !entry.path().join("initstate").exists())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .map(|name| driver(&name, None, &catalog)),
    );
    drivers.sort_by(|a, b| a.name.cmp(&b.name));
    drivers
}

fn driver(name: &str, loaded: Option<&LoadedLine>, catalog: &HashMap<String, Entry>) -> Driver {
    let module = Path::new(SYS_MODULE).join(name);
    let entry = catalog.get(name);
    Driver {
        name: name.to_owned(),
        size: loaded.map(|line| line.size),
        state: loaded.map(|line| line.state),
        used_by: loaded.map_or_else(Vec::new, |line| line.used_by.clone()),
        source_version: sysfs::read(module.join("srcversion")),
        taint: sysfs::read::<String>(module.join("taint")).filter(|taint| !taint.is_empty()),
        parameters: fs::read_dir(module.join("parameters")).map_or(0, Iterator::count),
        path: entry.map(|entry| entry.path.clone()),
        depends: entry.map_or_else(Vec::new, |entry| entry.depends.clone()),
    }
}

struct LoadedLine {
    name: String,
    size: u64,
    used_by: Vec<String>,
    state: State,
}

/// `name size refcount used_by,  state address`; `used_by` is `-` when nothing uses the module.
fn parse_loaded(line: &str) -> Option<LoadedLine> {
    let mut fields = line.split_ascii_whitespace();
    let name = fields.next()?.to_owned();
    let size = fields.next()?.parse().ok()?;
    let used_by = fields.nth(1)?;
    let state = match fields.next()? {
        "Live" => State::Live,
        "Loading" => State::Loading,
        "Unloading" => State::Unloading,
        _ => return None,
    };
    Some(LoadedLine {
        name,
        size,
        used_by: used_by
            .split(',')
            .filter(|user| !matches!(*user, "" | "-"))
            .map(str::to_owned)
            .collect(),
        state,
    })
}

struct Entry {
    path: PathBuf,
    depends: Vec<String>,
}

/// Where each module's file is and what it depends on, from the kernel's `modules.dep`.
fn catalog(release: &str) -> Option<HashMap<String, Entry>> {
    let directory = [
        format!("/lib/modules/{release}"),
        format!("/run/booted-system/kernel-modules/lib/modules/{release}"),
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|directory| directory.join("modules.dep").exists())?;
    let text = fs::read_to_string(directory.join("modules.dep")).ok()?;
    Some(parse_catalog(&text, &directory))
}

/// `kernel/path/name.ko.xz: kernel/path/dependency.ko.xz ...`
fn parse_catalog(text: &str, directory: &Path) -> HashMap<String, Entry> {
    text.lines()
        .filter_map(|line| {
            let (file, depends) = line.split_once(':')?;
            Some((
                module_name(file)?,
                Entry {
                    path: directory.join(file),
                    depends: depends
                        .split_ascii_whitespace()
                        .filter_map(module_name)
                        .collect(),
                },
            ))
        })
        .collect()
}

/// The module name in a file path: the stem before `.ko`, with dashes as underscores like the kernel.
fn module_name(file: &str) -> Option<String> {
    let name = Path::new(file).file_name()?.to_str()?;
    Some(name.split_once(".ko")?.0.replace('-', "_"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loaded_lines_split_users_and_state() {
        let line = parse_loaded(
            "snd_hda_core 126976 3 snd_hda_codec,snd_hda_intel, Live 0x0000000000000000",
        )
        .unwrap();
        assert_eq!(
            (line.name.as_str(), line.size, line.state),
            ("snd_hda_core", 126_976, State::Live)
        );
        assert_eq!(line.used_by, ["snd_hda_codec", "snd_hda_intel"]);
        assert_eq!(
            parse_loaded("ib_core 675840 0 - Live 0x0").unwrap().used_by,
            Vec::<String>::new()
        );
        assert!(parse_loaded("odd 1 0 - Strange 0x0").is_none());
    }

    #[test]
    fn module_names_ignore_compression_and_dashes() {
        assert_eq!(
            module_name("kernel/drivers/gpu/amdgpu.ko.zst").as_deref(),
            Some("amdgpu")
        );
        assert_eq!(
            module_name("kernel/net/nf-tables.ko").as_deref(),
            Some("nf_tables")
        );
        assert_eq!(module_name("modules.builtin"), None);
    }

    #[test]
    fn the_catalog_maps_names_to_files_and_dependencies() {
        let text = "kernel/a/one.ko.xz: kernel/b/two-x.ko.xz kernel/c/three.ko.xz\nkernel/b/two-x.ko.xz:\n";
        let catalog = parse_catalog(text, Path::new("/m"));
        assert_eq!(catalog["one"].path, Path::new("/m/kernel/a/one.ko.xz"));
        assert_eq!(catalog["one"].depends, ["two_x", "three"]);
        assert_eq!(catalog["two_x"].depends, Vec::<String>::new());
    }

    #[test]
    fn the_running_kernel_lists_loaded_and_built_in_modules() {
        let drivers = read();
        assert!(
            drivers
                .iter()
                .any(|driver| !driver.built_in() && driver.size.is_some())
        );
        assert!(drivers.iter().any(Driver::built_in));
        assert!(drivers.windows(2).all(|pair| pair[0].name <= pair[1].name));
    }
}
