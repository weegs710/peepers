use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use rustix::fs::statvfs;

use crate::math::percent;

/// A mounted filesystem with storage behind it. Sizes are in bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    pub source: String,
    pub mount: PathBuf,
    pub filesystem: String,
    pub total: u64,
    /// Free space an unprivileged user can use, which can be less than the filesystem's own free space.
    pub available: u64,
}

impl Volume {
    #[must_use]
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.available)
    }

    #[must_use]
    pub fn used_percent(&self) -> f32 {
        percent(self.used(), self.total)
    }
}

/// One entry of `/proc/self/mountinfo`.
struct Mount {
    device: String,
    root: PathBuf,
    point: PathBuf,
    filesystem: String,
    source: String,
}

/// Mounted filesystems that have a size. Kernel pseudo-filesystems report none, a bind mount of
/// part of a filesystem, or a second mount of one already listed, adds nothing, and system
/// plumbing such as `/dev` and the per-service mounts under `/run` is not storage anyone manages.
#[must_use]
pub fn read() -> Vec<Volume> {
    let mut seen = HashSet::new();
    parse(&mountinfo())
        .filter(|mount| {
            mount.root == Path::new("/")
                && !plumbing(&mount.point)
                && seen.insert(mount.device.clone())
        })
        .filter_map(|mount| {
            let stat = statvfs(&mount.point).ok()?;
            (stat.f_blocks > 0).then(|| Volume {
                source: mount.source,
                filesystem: mount.filesystem,
                total: stat.f_blocks * stat.f_frsize,
                available: stat.f_bavail * stat.f_frsize,
                mount: mount.point,
            })
        })
        .collect()
}

/// The filesystem that holds a path, which for a folder on a bind mount is the filesystem the
/// mount was taken from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backing {
    pub filesystem: String,
    pub source: String,
    pub mount: PathBuf,
}

/// What `path` is stored on: the deepest mount that contains it.
#[must_use]
pub fn backing(path: &Path) -> Option<Backing> {
    let path = fs::canonicalize(path).ok()?;
    deepest(parse(&mountinfo()), &path)
}

/// Paths and sources are whatever bytes the mounter chose, so the table need not be UTF-8.
fn mountinfo() -> Vec<u8> {
    fs::read("/proc/self/mountinfo").unwrap_or_default()
}

fn parse(mountinfo: &[u8]) -> impl Iterator<Item = Mount> {
    mountinfo
        .split(|&byte| byte == b'\n')
        .filter_map(parse_mount)
}

fn deepest(mounts: impl Iterator<Item = Mount>, path: &Path) -> Option<Backing> {
    mounts
        .filter(|mount| path.starts_with(&mount.point))
        .max_by_key(|mount| mount.point.components().count())
        .map(|mount| Backing {
            filesystem: mount.filesystem,
            source: mount.source,
            mount: mount.point,
        })
}

fn plumbing(point: &Path) -> bool {
    ["/dev", "/proc", "/sys"]
        .iter()
        .any(|prefix| point.starts_with(prefix))
        || (point.starts_with("/run") && point != Path::new("/run"))
}

/// `id parent major:minor root mount options [tags...] - filesystem source super-options`
fn parse_mount(line: &[u8]) -> Option<Mount> {
    let separator = line.windows(3).position(|window| window == b" - ")?;
    let mut head = fields(&line[..separator]).skip(2);
    let mut tail = fields(&line[separator + 3..]);
    Some(Mount {
        device: text(head.next()?),
        root: path(head.next()?),
        point: path(head.next()?),
        filesystem: text(tail.next()?),
        source: text(tail.next()?),
    })
}

fn fields(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    bytes
        .split(u8::is_ascii_whitespace)
        .filter(|field| !field.is_empty())
}

fn text(field: &[u8]) -> String {
    String::from_utf8_lossy(field).into_owned()
}

fn path(field: &[u8]) -> PathBuf {
    PathBuf::from(OsString::from_vec(unescape(field)))
}

/// The kernel writes spaces, tabs, newlines and backslashes in paths as `\040`-style octal.
fn unescape(path: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(path.len());
    let mut rest = path;
    while let Some((&byte, tail)) = rest.split_first() {
        let octal = (byte == b'\\')
            .then(|| tail.get(..3))
            .flatten()
            .and_then(|digits| u8::from_str_radix(std::str::from_utf8(digits).ok()?, 8).ok());
        if let Some(decoded) = octal {
            bytes.push(decoded);
            rest = &tail[3..];
        } else {
            bytes.push(byte);
            rest = tail;
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_lines_split_around_the_separator() {
        let mount =
            parse_mount(b"47 41 0:35 / /persist rw,noatime shared:6 - zfs zroot/persist rw,xattr")
                .unwrap();
        assert_eq!(mount.device, "0:35");
        assert_eq!(mount.root, PathBuf::from("/"));
        assert_eq!(mount.point, PathBuf::from("/persist"));
        assert_eq!(
            (mount.filesystem.as_str(), mount.source.as_str()),
            ("zfs", "zroot/persist")
        );
    }

    #[test]
    fn a_folder_is_stored_on_the_deepest_mount_that_contains_it() {
        let mountinfo = b"\
20 1 0:20 / / rw - tmpfs tmpfs rw\n\
47 20 0:35 / /persist rw - zfs zroot/persist rw\n\
48 20 0:35 /home/weegs/.claude /home/weegs/.claude rw - zfs zroot/persist[/home/weegs/.claude] rw\n";
        let on =
            |path| deepest(parse(mountinfo), Path::new(path)).map(|b| (b.filesystem, b.source));
        assert_eq!(
            on("/home/weegs/notes"),
            Some(("tmpfs".into(), "tmpfs".into()))
        );
        assert_eq!(
            on("/persist/data"),
            Some(("zfs".into(), "zroot/persist".into()))
        );
        assert_eq!(
            on("/home/weegs/.claude/x"),
            Some(("zfs".into(), "zroot/persist[/home/weegs/.claude]".into()))
        );
    }

    #[test]
    fn a_mount_whose_path_is_not_utf8_keeps_its_bytes_and_every_other_mount() {
        use std::os::unix::ffi::OsStrExt;
        let mountinfo = b"\
20 1 0:20 / / rw - tmpfs tmpfs rw\n\
61 20 0:61 / /mnt/odd\xff\\040name rw - fuse.sshfs host:\xff rw\n\
47 20 0:35 / /persist rw - zfs zroot/persist rw\n";
        let mounts: Vec<Mount> = parse(mountinfo).collect();
        assert_eq!(mounts.len(), 3);
        assert_eq!(mounts[1].point.as_os_str().as_bytes(), b"/mnt/odd\xff name");
        assert_eq!(mounts[1].source, "host:\u{fffd}");
        assert_eq!(mounts[2].point, PathBuf::from("/persist"));
    }

    #[test]
    fn this_folder_is_stored_somewhere() {
        assert!(backing(Path::new(env!("CARGO_MANIFEST_DIR"))).is_some());
    }

    #[test]
    fn plumbing_mounts_are_not_storage() {
        assert!(plumbing(Path::new("/dev/shm")) && plumbing(Path::new("/run/wrappers")));
        assert!(plumbing(Path::new("/sys/firmware/efi/efivars")));
        assert!(
            !plumbing(Path::new("/run"))
                && !plumbing(Path::new("/persist"))
                && !plumbing(Path::new("/"))
        );
    }

    #[test]
    fn octal_escapes_become_characters() {
        assert_eq!(unescape(br"/mnt/my\040disk"), b"/mnt/my disk");
        assert_eq!(unescape(br"/odd\134name"), br"/odd\name");
        assert_eq!(unescape(br"/trailing\04"), br"/trailing\04");
    }

    #[test]
    fn this_machine_lists_its_root_with_a_size() {
        let volumes = read();
        let root = volumes
            .iter()
            .find(|volume| volume.mount.as_os_str() == "/")
            .unwrap();
        assert!(root.total > 0 && root.used() <= root.total);
        let mounts: HashSet<_> = volumes.iter().map(|volume| &volume.mount).collect();
        assert_eq!(mounts.len(), volumes.len());
    }
}
