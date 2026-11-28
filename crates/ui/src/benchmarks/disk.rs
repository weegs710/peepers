use crate::theme::Element;
use crate::theme::styles;
use std::fmt;
use std::path::PathBuf;

use iced::widget::{column, pick_list, row, text, text_input};
use iced::{Fill, Task};
use peepers_core::bench::{Disk, Job, Medium, Outcome};
use peepers_core::volumes::{self, Volume};

use super::{Benchmarks, Message as Root, Section, Start, figure, not_run, page};
use crate::blocking;
use crate::format;
use crate::storage;

const INTRO: &str = "Writes one temporary file in the folder below, reads it back, and times small random reads, \
then deletes the file. The file exists only for the test and is removed when the test ends, even if it fails or you stop it. \
It never uses the last gibibyte of free space, and nothing else on the disk is touched.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Small,
    Medium,
    Large,
}

impl Size {
    const ALL: [Self; 3] = [Self::Small, Self::Medium, Self::Large];

    fn bytes(self) -> u64 {
        match self {
            Self::Small => 256 << 20,
            Self::Medium => 1 << 30,
            Self::Large => 4 << 30,
        }
    }
}

impl fmt::Display for Size {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&format::bytes(self.bytes()))
    }
}

/// A mounted volume the test folder can be pointed at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry(Volume);

impl fmt::Display for Entry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} ({}, {} free)",
            self.0.mount.display(),
            self.0.filesystem,
            format::bytes(self.0.available)
        )
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Directory(String),
    Size(Size),
    Volumes(Vec<Volume>),
    Volume(Entry),
}

#[derive(Debug)]
pub struct State {
    directory: String,
    size: Size,
    volumes: Vec<Entry>,
    loaded: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            directory: storage::default_folder(),
            size: Size::Medium,
            volumes: Vec::new(),
            loaded: false,
        }
    }
}

impl State {
    pub fn load(&self) -> Task<Message> {
        if self.loaded {
            return Task::none();
        }
        blocking::run(volumes::read).map(Message::Volumes)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Directory(directory) => self.directory = directory,
            Message::Size(size) => self.size = size,
            Message::Volumes(volumes) => {
                self.volumes = volumes.into_iter().map(Entry).collect();
                self.loaded = true;
            }
            Message::Volume(entry) => self.directory = entry.0.mount.display().to_string(),
        }
        Task::none()
    }

    pub fn job(&self) -> Job {
        Job::Disk {
            directory: PathBuf::from(&self.directory),
            size: self.size.bytes(),
        }
    }

    /// The volume the folder is on: the one mounted deepest among those that contain it.
    fn selected(&self) -> Option<&Entry> {
        let folder = PathBuf::from(&self.directory);
        self.volumes
            .iter()
            .filter(|entry| folder.starts_with(&entry.0.mount))
            .max_by_key(|entry| entry.0.mount.components().count())
    }
}

pub(super) fn view(app: &Benchmarks) -> Element<'_, Root> {
    let state = &app.disk;
    let controls = column![
        row![
            pick_list(state.volumes.as_slice(), state.selected(), |entry| {
                Root::Disk(Message::Volume(entry))
            })
            .placeholder("Pick a disk")
            .width(Fill),
            pick_list(Size::ALL, Some(state.size), |size| Root::Disk(
                Message::Size(size)
            )),
        ]
        .spacing(8),
        row![
            text_input("Folder on the disk to test", &state.directory)
                .on_input(|directory| Root::Disk(Message::Directory(directory)))
                .width(Fill),
            app.run_button("Run", Start::One(Section::Disk)),
        ]
        .spacing(8),
    ]
    .spacing(8);
    page(
        "Disk",
        INTRO,
        controls.into(),
        app.latest(|outcome| match outcome {
            Outcome::Disk(disk) => Some(disk),
            _ => None,
        })
        .map_or_else(not_run, results),
    )
}

fn results(disk: &Disk) -> Element<'_, Root> {
    let (caveat, failed) = caveat(disk);
    column![
        text(format!(
            "Tested on {} ({}) with a {} file",
            disk.filesystem,
            if disk.source.is_empty() { "unknown source" } else { &disk.source },
            format::bytes(disk.size)
        )),
        text(caveat)
            .size(13)
            .style(if failed { styles::text_danger } else { styles::text_secondary }),
        figure(
            "Sequential write",
            format::rate(disk.write),
            "Saving large files. Each write was flushed to the disk before the clock stopped. A short test mostly measures the drive's fast cache, and a long copy can fall below this once that cache fills.",
        ),
        figure(
            "Sequential read",
            format::rate(disk.read),
            "Loading large files, games and video.",
        ),
        figure(
            "Access time",
            format!(
                "{:.0} µs typical, {:.0} µs for the slowest one in a hundred",
                disk.access.median_us, disk.access.p99_us
            ),
            "How long the disk takes to answer one small read. This is what makes a computer feel quick or sluggish when it opens many small files.",
        ),
        figure(
            "Small reads, one at a time",
            format!("{} a second", disk.iops.single),
            "Random 4 KiB reads made one after another, each waiting for the last.",
        ),
        figure(
            "Small reads, many at once",
            format!("{} a second with {} in flight", disk.iops.multi, disk.parallel),
            "The same reads with many requests outstanding, as when several programs use the disk together. SSDs gain a great deal here and hard disks hardly at all.",
        ),
    ]
    .spacing(14)
    .into()
}

/// What the reader needs to know about whether the figures describe the disk, and whether that
/// is a problem.
fn caveat(disk: &Disk) -> (String, bool) {
    match (disk.medium, disk.direct) {
        (Medium::Memory, _) => (
            "This folder is stored in memory, so these are memory speeds and not a disk's. Pick a folder on a real disk.".to_owned(),
            true,
        ),
        (Medium::Network, _) => (
            "This folder is on another machine, so the figures include the network between you and it.".to_owned(),
            false,
        ),
        (Medium::Storage, true) => (
            "Reads and writes went around the system's caches, so these figures belong to the storage itself, and can be lower than a file copy that looks instant.".to_owned(),
            false,
        ),
        (Medium::Storage, false) => (
            format!(
                "{} would not let the test skip the system's cache, so reads may have come from memory and flatter the disk. Writes were still flushed to the disk before the clock stopped.",
                disk.filesystem
            ),
            true,
        ),
    }
}

pub(super) fn summary(disk: &Disk) -> String {
    format!(
        "{}: write {}, read {}, {:.0} µs access, {} small reads a second ({} with {} at once){}",
        disk.filesystem,
        format::rate(disk.write),
        format::rate(disk.read),
        disk.access.median_us,
        disk.iops.single,
        disk.iops.multi,
        disk.parallel,
        if disk.direct { "" } else { ", cached" }
    )
}

#[cfg(test)]
mod tests {
    use peepers_core::bench::{Access, Pair};

    use super::*;

    fn measured() -> Disk {
        Disk {
            filesystem: "zfs".to_owned(),
            source: "zroot/persist".to_owned(),
            medium: Medium::Storage,
            size: 1 << 30,
            direct: true,
            write: 1,
            read: 1,
            access: Access {
                median_us: 100.0,
                p99_us: 200.0,
            },
            iops: Pair {
                single: 5,
                multi: 50,
            },
            parallel: 16,
        }
    }

    fn volume(mount: &str) -> Entry {
        Entry(Volume {
            source: "s".to_owned(),
            mount: PathBuf::from(mount),
            filesystem: "zfs".to_owned(),
            total: 10,
            available: 5,
        })
    }

    #[test]
    fn sizes_read_in_binary_units() {
        assert_eq!(Size::Small.to_string(), "256.0 MiB");
        assert_eq!(Size::Large.to_string(), "4.0 GiB");
    }

    #[test]
    fn the_folder_is_matched_to_the_deepest_volume_that_holds_it() {
        let mut state = State {
            volumes: vec![volume("/"), volume("/home"), volume("/home/weegs/.cache")],
            directory: "/home/weegs/.cache/peepers".to_owned(),
            ..State::default()
        };
        assert_eq!(state.selected(), Some(&state.volumes[2]));
        state.directory = "/home/weegs/Documents".to_owned();
        assert_eq!(state.selected(), Some(&state.volumes[1]));
    }

    #[test]
    fn picking_a_volume_points_the_folder_at_it() {
        let mut state = State::default();
        drop(state.update(Message::Volume(volume("/persist"))));
        assert_eq!(state.directory, "/persist");
    }

    #[test]
    fn the_job_carries_the_folder_and_the_size() {
        let state = State {
            directory: "/data".to_owned(),
            ..State::default()
        };
        assert_eq!(
            state.job(),
            Job::Disk {
                directory: PathBuf::from("/data"),
                size: 1 << 30
            }
        );
    }

    #[test]
    fn memory_is_called_out_as_a_problem_and_a_direct_disk_is_not() {
        let mut disk = measured();
        disk.medium = Medium::Memory;
        assert!(caveat(&disk).1);
        disk.medium = Medium::Storage;
        disk.direct = true;
        assert!(!caveat(&disk).1);
        disk.direct = false;
        assert!(caveat(&disk).0.contains("zfs") && caveat(&disk).1);
    }

    #[test]
    fn a_summary_says_when_the_reads_were_cached() {
        let mut disk = measured();
        assert!(!summary(&disk).ends_with("cached"));
        disk.direct = false;
        assert!(summary(&disk).ends_with("cached"));
    }
}
