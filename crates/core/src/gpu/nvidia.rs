use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::{Pool, pci_address};

#[derive(Debug, Clone)]
pub(super) struct Metrics {
    pub address: String,
    pub busy: Option<f32>,
    pub vram: Option<Pool>,
}

pub(super) fn read() -> Vec<Metrics> {
    query().map_or_else(Vec::new, |text| parse(&text))
}

fn query() -> Option<String> {
    // Query every NVIDIA card once, matching PCI addresses rather than driver/card indices.
    let mut child = Command::new("nvidia-smi")
        .args([
            "--query-gpu=pci.bus_id,utilization.gpu,memory.total,memory.used",
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
            let [address, busy, total, used] = fields.as_slice() else {
                return None;
            };
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
    fn rejects_malformed_output_and_nonfinite_or_out_of_range_usage() {
        assert!(parse("NVIDIA-SMI failed\nnot-a-slot, 0, 1, 0").is_empty());
        for busy in ["NaN", "inf", "101", "-1"] {
            assert_eq!(parse(&format!("0000:01:00.0, {busy}, 0, 0"))[0].busy, None);
        }
    }
}
