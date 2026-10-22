use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::{Pool, pci_address};
use crate::sensors::{Kind, Reading};

/// The chip name on the readings, which the closed driver has no hwmon chip to supply.
pub(super) const CHIP: &str = "nvidia";

/// What every driver this reader has met answers to.
const BASE: &str = "pci.bus_id,utilization.gpu,memory.total,memory.used";
/// What a card reports about itself beyond use and memory, in the order [`SENSORS`] names them.
const EXTRA: &str = "temperature.gpu,temperature.memory,power.draw,clocks.sm,clocks.mem";

const SENSORS: [(&str, Kind); 5] = [
    ("temperature", Kind::Temperature),
    ("memory temperature", Kind::Temperature),
    ("power draw", Kind::Power),
    ("graphics clock", Kind::Frequency),
    ("memory clock", Kind::Frequency),
];

#[derive(Debug, Clone)]
pub(super) struct Metrics {
    pub address: String,
    pub busy: Option<f32>,
    pub vram: Option<Pool>,
    pub sensors: Vec<Reading>,
}

/// Asks for the sensors too, and asks again for use and memory alone when the longer question
/// brings back no card, so a field the driver does not know can never cost what already worked.
pub(super) fn read() -> Vec<Metrics> {
    let asked = |fields: &str| query(fields).map(|text| parse(&text)).unwrap_or_default();
    let cards = asked(&format!("{BASE},{EXTRA}"));
    if cards.is_empty() { asked(BASE) } else { cards }
}

fn query(fields: &str) -> Option<String> {
    // Query every NVIDIA card once, matching PCI addresses rather than driver/card indices.
    let mut child = Command::new("nvidia-smi")
        .args([
            &format!("--query-gpu={fields}"),
            "--format=csv,noheader,nounits",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child.wait_with_output().ok()?;
                return status
                    .success()
                    .then(|| String::from_utf8_lossy(&output.stdout).into_owned());
            }
            Ok(None) if started.elapsed() < Duration::from_millis(500) => {
                std::thread::sleep(Duration::from_millis(5));
            }
            _ => {
                // A broken driver must not indefinitely stall all telemetry.
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

fn parse(text: &str) -> Vec<Metrics> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<_> = line.split(',').map(str::trim).collect();
            let [address, busy, total, used, extra @ ..] = fields.as_slice() else {
                return None;
            };
            if !extra.is_empty() && extra.len() != SENSORS.len() {
                return None;
            }
            let mib = |text: &str| text.parse::<u64>().ok()?.checked_mul(1 << 20);
            Some(Metrics {
                address: pci_address(address)?,
                busy: busy
                    .parse::<f32>()
                    .ok()
                    .filter(|value| value.is_finite() && (0.0..=100.0).contains(value)),
                vram: mib(total)
                    .zip(mib(used))
                    .and_then(|(total, used)| (total > 0).then_some(Pool { total, used })),
                sensors: extra
                    .iter()
                    .zip(SENSORS)
                    .filter_map(|(text, (label, kind))| {
                        let value = text.parse::<f32>().ok().filter(|value| value.is_finite())?;
                        (value >= 0.0).then(|| Reading::measured(CHIP, label, kind, value))
                    })
                    .collect(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_pci_addresses_and_converts_mib_to_bytes() {
        let metrics = parse("00000000:01:00.0, 37, 8151, 123\n00000000:02:00.0, 0, 4096, 0\n");
        assert_eq!(metrics.len(), 2);
        assert_eq!(metrics[0].address, "0000:01:00.0");
        assert_eq!(metrics[0].busy, Some(37.0));
        assert_eq!(
            metrics[0].vram,
            Some(Pool {
                total: 8151 << 20,
                used: 123 << 20
            })
        );
        assert_eq!(metrics[1].busy, Some(0.0));
    }

    #[test]
    fn unavailable_fields_do_not_discard_supported_metrics() {
        let metrics =
            parse("00000000:01:00.0, [N/A], 8192, 12\n0000:02:00.0, 45, [Not Supported], [N/A]\n");
        assert_eq!(metrics[0].busy, None);
        assert!(metrics[0].vram.is_some());
        assert_eq!(metrics[1].busy, Some(45.0));
        assert_eq!(metrics[1].vram, None);
    }

    #[test]
    fn the_sensors_of_a_card_are_read_in_their_units() {
        let metrics = parse("00000000:01:00.0, 37, 8151, 123, 61, 58, 142.50, 1950, 7001\n");
        let found: Vec<_> = metrics[0]
            .sensors
            .iter()
            .map(|reading| (reading.label.as_str(), reading.kind, reading.value()))
            .collect();
        let expected = [
            ("temperature", Kind::Temperature, 61.0),
            ("memory temperature", Kind::Temperature, 58.0),
            ("power draw", Kind::Power, 142.5),
            ("graphics clock", Kind::Frequency, 1950.0),
            ("memory clock", Kind::Frequency, 7001.0),
        ];
        assert_eq!(found.len(), expected.len());
        for ((label, kind, value), (want_label, want_kind, want)) in found.into_iter().zip(expected)
        {
            assert_eq!((label, kind), (want_label, want_kind));
            assert!((value - want).abs() < 1e-3, "{label} read {value}");
        }
    }

    #[test]
    fn a_sensor_the_card_does_not_have_is_left_out_and_the_rest_stay() {
        let metrics =
            parse("00000000:01:00.0, 5, 8192, 12, 44, [N/A], [Not Supported], 210, N/A\n");
        let labels: Vec<_> = metrics[0]
            .sensors
            .iter()
            .map(|reading| reading.label.as_str())
            .collect();
        assert_eq!(labels, ["temperature", "graphics clock"]);
        assert_eq!(metrics[0].busy, Some(5.0));
    }

    #[test]
    fn the_shorter_answer_still_gives_use_and_memory_and_no_sensors() {
        let metrics = parse("00000000:01:00.0, 37, 8151, 123\n");
        assert_eq!(metrics[0].busy, Some(37.0));
        assert_eq!(metrics[0].sensors, []);
    }

    #[test]
    fn a_line_with_some_but_not_all_of_the_sensor_columns_is_malformed() {
        assert!(parse("00000000:01:00.0, 37, 8151, 123, 61, 58\n").is_empty());
    }

    #[test]
    fn rejects_malformed_output_and_nonfinite_or_out_of_range_usage() {
        assert!(parse("NVIDIA-SMI failed\nnot-a-slot, 0, 1, 0").is_empty());
        for busy in ["NaN", "inf", "101", "-1"] {
            assert_eq!(parse(&format!("0000:01:00.0, {busy}, 0, 0"))[0].busy, None);
        }
    }
}
