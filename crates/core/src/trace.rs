use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::network::Kind;
use crate::telemetry::Vitals;

pub const FORMAT: &str = "peepers-trace";
pub const VERSION: u32 = 1;

/// Two days at one sample per second.
const MAX_FRAMES: usize = 172_800;
const MAX_FILE_BYTES: u64 = 256 << 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Channel {
    pub id: String,
    pub label: String,
    pub unit: String,
}

/// What was recorded and how each frame is laid out: frame arrays follow these lists in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub format: String,
    pub version: u32,
    /// Seconds since the Unix epoch when recording began.
    pub recorded_at: u64,
    pub machine: String,
    pub kernel: String,
    pub memory_total: u64,
    pub cores: usize,
    pub disks: Vec<String>,
    pub interfaces: Vec<String>,
    pub gpus: Vec<String>,
    pub sensors: Vec<Channel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DiskFrame {
    pub active: f32,
    pub read: u64,
    pub write: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkFrame {
    pub receive: u64,
    pub send: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GpuFrame {
    pub busy: f32,
    /// Video memory in use, as a percentage of what the adapter has.
    pub memory: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    /// Seconds since the first frame.
    pub seconds: f32,
    pub cpu: f32,
    pub cores: Vec<f32>,
    pub memory: f32,
    pub disks: Vec<DiskFrame>,
    pub interfaces: Vec<NetworkFrame>,
    pub gpus: Vec<GpuFrame>,
    pub sensors: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trace {
    pub header: Header,
    pub frames: Vec<Frame>,
}

/// Records [`Vitals`] over time. The set of devices is fixed by the first sample, so a device
/// that appears later is not recorded and one that disappears reads as zero.
#[derive(Debug)]
pub struct Recorder {
    header: Header,
    frames: Vec<Frame>,
    started: Instant,
}

impl Recorder {
    #[must_use]
    pub fn start(first: &Vitals) -> Self {
        let text = |path: &str| {
            fs::read_to_string(path)
                .map(|text| text.trim().to_owned())
                .unwrap_or_default()
        };
        let header = Header {
            format: FORMAT.to_owned(),
            version: VERSION,
            recorded_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_secs()),
            machine: text("/proc/sys/kernel/hostname"),
            kernel: text("/proc/sys/kernel/osrelease"),
            memory_total: first.memory.total,
            cores: first.cpu.cores.len(),
            disks: first.disks.iter().map(|disk| disk.name.clone()).collect(),
            interfaces: first
                .interfaces
                .iter()
                .filter(|interface| recorded(interface.kind))
                .map(|interface| interface.name.clone())
                .collect(),
            gpus: first
                .adapters
                .iter()
                .map(|adapter| adapter.card.clone())
                .collect(),
            sensors: first
                .chips
                .iter()
                .flat_map(|chip| {
                    chip.readings
                        .iter()
                        .map(|reading| channel(&chip.id, reading))
                })
                .collect(),
        };
        let frames = vec![frame(&header, first, 0.0)];
        Self {
            header,
            frames,
            started: Instant::now(),
        }
    }

    /// Adds a frame. Returns `false` once the recording has reached its limit.
    pub fn record(&mut self, vitals: &Vitals) -> bool {
        if self.frames.len() >= MAX_FRAMES {
            return false;
        }
        self.frames.push(frame(
            &self.header,
            vitals,
            self.started.elapsed().as_secs_f32(),
        ));
        true
    }

    #[must_use]
    pub fn frames(&self) -> usize {
        self.frames.len()
    }

    #[must_use]
    pub fn finish(self) -> Trace {
        Trace {
            header: self.header,
            frames: self.frames,
        }
    }
}

fn recorded(kind: Kind) -> bool {
    matches!(kind, Kind::Ethernet | Kind::Wireless)
}

fn channel(chip: &str, reading: &crate::sensors::Reading) -> Channel {
    Channel {
        id: format!("{chip}/{}", reading.label),
        label: reading.label.clone(),
        unit: reading.kind.unit().to_owned(),
    }
}

fn frame(header: &Header, vitals: &Vitals, seconds: f32) -> Frame {
    Frame {
        seconds,
        cpu: finite(vitals.cpu.usage),
        cores: (0..header.cores)
            .map(|index| {
                vitals
                    .cpu
                    .cores
                    .get(index)
                    .map_or(0.0, |core| finite(core.usage))
            })
            .collect(),
        memory: finite(vitals.memory.used_percent()),
        disks: header
            .disks
            .iter()
            .map(|name| {
                vitals.disks.iter().find(|disk| &disk.name == name).map_or(
                    DiskFrame {
                        active: 0.0,
                        read: 0,
                        write: 0,
                    },
                    |disk| DiskFrame {
                        active: finite(disk.active),
                        read: disk.read_per_second,
                        write: disk.write_per_second,
                    },
                )
            })
            .collect(),
        interfaces: header
            .interfaces
            .iter()
            .map(|name| {
                vitals
                    .interfaces
                    .iter()
                    .find(|interface| &interface.name == name)
                    .map_or(
                        NetworkFrame {
                            receive: 0,
                            send: 0,
                        },
                        |interface| NetworkFrame {
                            receive: interface.receive_per_second,
                            send: interface.send_per_second,
                        },
                    )
            })
            .collect(),
        gpus: header
            .gpus
            .iter()
            .map(|card| {
                vitals
                    .adapters
                    .iter()
                    .find(|adapter| &adapter.card == card)
                    .map_or(
                        GpuFrame {
                            busy: 0.0,
                            memory: 0.0,
                        },
                        |adapter| GpuFrame {
                            busy: adapter.busy.map_or(0.0, finite),
                            memory: adapter.vram.map_or(0.0, |pool| {
                                finite(crate::math::percent(pool.used, pool.total))
                            }),
                        },
                    )
            })
            .collect(),
        sensors: header
            .sensors
            .iter()
            .map(|channel| {
                vitals
                    .chips
                    .iter()
                    .flat_map(|chip| chip.readings.iter().map(move |reading| (chip, reading)))
                    .find(|(chip, reading)| self::channel(&chip.id, reading).id == channel.id)
                    .map_or(0.0, |(_, reading)| finite(reading.value()))
            })
            .collect(),
    }
}

/// JSON has no way to write a NaN or an infinity, so they are recorded as zero.
fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

/// Why a trace cannot be used.
#[derive(Debug, PartialEq, Eq)]
pub enum Invalid {
    Format,
    Version(u32),
    NoFrames,
    TooManyFrames,
    /// The first frame is not at zero seconds, or a frame is not later than the one before.
    Time(usize),
    /// A frame does not have the arrays the header describes.
    Shape(usize),
}

impl fmt::Display for Invalid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format => write!(formatter, "This file is not a {FORMAT} recording."),
            Self::Version(version) if *version > VERSION => {
                write!(
                    formatter,
                    "This recording was written by a newer version (format {version})."
                )
            }
            Self::Version(version) => write!(
                formatter,
                "This recording has an unsupported format version ({version})."
            ),
            Self::NoFrames => write!(formatter, "This recording contains no samples."),
            Self::TooManyFrames => write!(
                formatter,
                "This recording contains more than {MAX_FRAMES} samples."
            ),
            Self::Time(frame) => write!(formatter, "The timestamps are invalid at sample {frame}."),
            Self::Shape(frame) => write!(
                formatter,
                "Sample {frame} does not match the recording's devices."
            ),
        }
    }
}

impl Trace {
    /// # Errors
    ///
    /// Fails with the first way the trace is inconsistent with its own header.
    pub fn validate(&self) -> Result<(), Invalid> {
        let header = &self.header;
        if header.format != FORMAT {
            return Err(Invalid::Format);
        }
        if header.version != VERSION {
            return Err(Invalid::Version(header.version));
        }
        if self.frames.is_empty() {
            return Err(Invalid::NoFrames);
        }
        if self.frames.len() > MAX_FRAMES {
            return Err(Invalid::TooManyFrames);
        }
        let mut earlier = None;
        for (index, frame) in self.frames.iter().enumerate() {
            let ordered = match earlier {
                None => frame.seconds == 0.0,
                Some(earlier) => frame.seconds > earlier,
            };
            if !ordered || !frame.seconds.is_finite() {
                return Err(Invalid::Time(index));
            }
            earlier = Some(frame.seconds);
            if frame.cores.len() != header.cores
                || frame.disks.len() != header.disks.len()
                || frame.interfaces.len() != header.interfaces.len()
                || frame.gpus.len() != header.gpus.len()
                || frame.sensors.len() != header.sensors.len()
            {
                return Err(Invalid::Shape(index));
            }
        }
        Ok(())
    }

    /// Writes the trace as a new file in `directory`, never replacing one that exists.
    ///
    /// # Errors
    ///
    /// Fails when the directory cannot be created or written, or the file is already there.
    pub fn save(&self, directory: &Path) -> io::Result<PathBuf> {
        fs::create_dir_all(directory)?;
        let path = directory.join(format!("peepers-{}.json", self.header.recorded_at));
        let partial = path.with_extension("json.partial");
        let mut writer = BufWriter::new(File::create_new(&partial)?);
        serde_json::to_writer(&mut writer, self).map_err(io::Error::from)?;
        writer.flush()?;
        if path.exists() {
            fs::remove_file(&partial)?;
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        fs::rename(&partial, &path)?;
        Ok(path)
    }

    /// # Errors
    ///
    /// Fails when the file is unreadable, too large, not JSON of this shape, or inconsistent.
    pub fn load(path: &Path) -> Result<Self, LoadError> {
        if fs::metadata(path)?.len() > MAX_FILE_BYTES {
            return Err(LoadError::TooLarge);
        }
        let trace: Self = serde_json::from_reader(BufReader::new(File::open(path)?))?;
        trace.validate()?;
        Ok(trace)
    }
}

#[derive(Debug)]
pub enum LoadError {
    Io(io::Error),
    TooLarge,
    Json(serde_json::Error),
    Invalid(Invalid),
}

impl fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "The recording could not be read: {error}."),
            Self::TooLarge => write!(
                formatter,
                "The recording is larger than {} MiB.",
                MAX_FILE_BYTES >> 20
            ),
            Self::Json(_) => write!(formatter, "The recording is not valid {FORMAT} data."),
            Self::Invalid(invalid) => invalid.fmt(formatter),
        }
    }
}

impl From<io::Error> for LoadError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for LoadError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<Invalid> for LoadError {
    fn from(invalid: Invalid) -> Self {
        Self::Invalid(invalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::Sampler;

    fn vitals() -> Vitals {
        Sampler::default().sample().unwrap().vitals
    }

    fn recording(frames: usize) -> Trace {
        let first = vitals();
        let mut recorder = Recorder::start(&first);
        for _ in 1..frames {
            std::thread::sleep(std::time::Duration::from_millis(2));
            recorder.record(&first);
        }
        recorder.finish()
    }

    #[test]
    fn a_fresh_recording_validates_and_starts_at_zero() {
        let trace = recording(3);
        assert_eq!(trace.validate(), Ok(()));
        assert!(trace.frames[0].seconds.abs() < f32::EPSILON);
        assert!(
            trace
                .frames
                .windows(2)
                .all(|pair| pair[1].seconds > pair[0].seconds)
        );
    }

    #[test]
    fn frames_follow_the_header_layout() {
        let trace = recording(2);
        let frame = &trace.frames[0];
        assert_eq!(frame.cores.len(), trace.header.cores);
        assert_eq!(frame.disks.len(), trace.header.disks.len());
        assert_eq!(frame.sensors.len(), trace.header.sensors.len());
        assert_eq!(trace.header.format, FORMAT);
    }

    #[test]
    fn json_round_trips() {
        let trace = recording(3);
        let text = serde_json::to_string(&trace).unwrap();
        let again: Trace = serde_json::from_str(&text).unwrap();
        assert_eq!(again, trace);
    }

    #[test]
    fn validation_names_the_first_fault() {
        let mut trace = recording(3);
        trace.frames[1].seconds = trace.frames[0].seconds;
        assert_eq!(trace.validate(), Err(Invalid::Time(1)));

        let mut trace = recording(2);
        trace.frames[1].cores.pop();
        assert_eq!(trace.validate(), Err(Invalid::Shape(1)));

        let mut trace = recording(2);
        trace.header.version = VERSION + 1;
        assert_eq!(trace.validate(), Err(Invalid::Version(VERSION + 1)));

        let mut trace = recording(1);
        trace.frames.clear();
        assert_eq!(trace.validate(), Err(Invalid::NoFrames));

        let mut trace = recording(1);
        trace.frames[0].seconds = 1.0;
        assert_eq!(trace.validate(), Err(Invalid::Time(0)));
    }

    #[test]
    fn the_limit_stops_a_recorder() {
        let first = vitals();
        let mut recorder = Recorder::start(&first);
        recorder
            .frames
            .resize(MAX_FRAMES, recorder.frames[0].clone());
        assert!(!recorder.record(&first));
        assert_eq!(recorder.frames(), MAX_FRAMES);
    }

    #[test]
    fn non_finite_values_are_recorded_as_zero() {
        assert!(
            finite(f32::NAN).abs() < f32::EPSILON && finite(f32::INFINITY).abs() < f32::EPSILON
        );
        assert!((finite(2.5) - 2.5).abs() < f32::EPSILON);
    }
}
