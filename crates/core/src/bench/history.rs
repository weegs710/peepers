use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::Outcome;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// Seconds since the Unix epoch when the benchmark finished.
    pub at: u64,
    pub outcome: Outcome,
}

/// Finished benchmarks, oldest first.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct History(Vec<Record>);

/// A history read back from its file.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Loaded {
    pub history: History,
    /// Results saved in a shape an older version used and this one cannot read, which are left out.
    pub skipped: usize,
}

impl History {
    /// A missing file is an empty history; a file that is not one is an error.
    ///
    /// # Errors
    ///
    /// Fails when the file exists but cannot be read, or is not a list.
    pub fn load(path: &Path) -> io::Result<Loaded> {
        match File::open(path) {
            Ok(file) => serde_json::from_reader(BufReader::new(file))
                .map(Loaded::from_values)
                .map_err(io::Error::from),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Loaded::default()),
            Err(error) => Err(error),
        }
    }

    /// Replaces the file in one step, so a crash never leaves half a history.
    ///
    /// # Errors
    ///
    /// Fails when the directory or file cannot be written.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(directory) = path.parent() {
            fs::create_dir_all(directory)?;
        }
        let partial = path.with_extension("json.partial");
        let mut writer = BufWriter::new(File::create(&partial)?);
        serde_json::to_writer(&mut writer, self).map_err(io::Error::from)?;
        writer.flush()?;
        fs::rename(partial, path)
    }

    pub fn push(&mut self, outcome: Outcome) {
        let at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        self.0.push(Record { at, outcome });
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Newest first.
    pub fn records(&self) -> impl Iterator<Item = &Record> {
        self.0.iter().rev()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Loaded {
    fn from_values(values: Vec<serde_json::Value>) -> Self {
        let total = values.len();
        let records: Vec<Record> = values
            .into_iter()
            .filter_map(|value| serde_json::from_value(value).ok())
            .collect();
        Self {
            skipped: total - records.len(),
            history: History(records),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::{Cpu, Pair};

    fn cpu(speed: f32) -> Outcome {
        let pair = Pair {
            single: speed,
            multi: speed * 8.0,
        };
        Outcome::Cpu(Cpu {
            threads: 8,
            integer: pair,
            float: pair,
            latencies: Vec::new(),
            bandwidth: Pair {
                single: 1,
                multi: 8,
            },
        })
    }

    #[test]
    fn records_list_newest_first() {
        let mut history = History::default();
        history.push(cpu(1.0));
        history.push(cpu(2.0));
        let points: Vec<_> = history
            .records()
            .map(|record| record.outcome.clone())
            .collect();
        assert_eq!(points, [cpu(2.0), cpu(1.0)]);
    }

    #[test]
    fn json_round_trips() {
        let mut history = History::default();
        history.push(cpu(3.5));
        let text = serde_json::to_string(&history).unwrap();
        assert_eq!(serde_json::from_str::<History>(&text).unwrap(), history);
    }

    #[test]
    fn a_missing_file_is_an_empty_history() {
        assert!(
            History::load(Path::new("/nonexistent/peepers/benchmarks.json"))
                .unwrap()
                .history
                .is_empty()
        );
    }

    #[test]
    fn results_in_an_older_shape_are_skipped_and_counted() {
        let current = serde_json::to_value(Record {
            at: 5,
            outcome: cpu(1.0),
        })
        .unwrap();
        let older = serde_json::json!({
            "at": 4,
            "outcome": { "Cpu": { "single": 182.2, "multi": 1613.0, "threads": 16 } }
        });
        let loaded = Loaded::from_values(vec![older, current]);
        assert_eq!(loaded.skipped, 1);
        assert_eq!(loaded.history.records().count(), 1);
    }

    #[test]
    fn clearing_forgets_everything() {
        let mut history = History::default();
        history.push(cpu(1.0));
        history.clear();
        assert!(history.is_empty());
    }
}
