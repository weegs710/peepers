//! Engine time reported by accessible DRM clients, deduplicated across processes and FDs.
//! See <https://www.kernel.org/doc/html/latest/gpu/drm-usage-stats.html>.
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use super::pci_address;
use crate::math::percent;

type Clients = HashMap<(String, u64), Client>;

#[derive(Debug, Default)]
struct Client {
    engines: HashMap<String, u64>,
    capacity: HashMap<String, u64>,
}

#[derive(Debug, Default)]
pub(super) struct Sampler {
    before: Clients,
    at: Option<Instant>,
}

impl Sampler {
    pub fn sample(&mut self) -> HashMap<String, f32> {
        self.update(read_clients(Path::new("/proc")), Instant::now())
    }

    fn update(&mut self, mut clients: Clients, now: Instant) -> HashMap<String, f32> {
        let elapsed = self.at.replace(now).map_or(Duration::ZERO, |at| now - at);
        let nanos = u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX);
        let mut engines: HashMap<(&str, &str), f32> = HashMap::new();
        if nanos > 0 {
            for (key, client) in &mut clients {
                let Some(before) = self.before.get(key) else {
                    continue;
                };
                for (engine, time) in &mut client.engines {
                    let Some(previous) = before.engines.get(engine) else {
                        continue;
                    };
                    // The ABI permits temporarily decreasing counters: retain the high-water mark.
                    *time = (*time).max(*previous);
                    let capacity = client.capacity.get(engine).copied().unwrap_or(1).max(1);
                    *engines.entry((&key.0, engine)).or_default() +=
                        percent(*time - previous, nanos.saturating_mul(capacity));
                }
            }
        }
        // Independent engines can overlap. Show the busiest engine class instead of summing them.
        let mut busy = HashMap::<String, f32>::new();
        for ((address, _), usage) in engines {
            let value = busy.entry(address.to_owned()).or_default();
            *value = value.max(usage.clamp(0.0, 100.0));
        }
        self.before = clients;
        busy
    }
}

fn read_clients(proc: &Path) -> Clients {
    let mut clients = Clients::new();
    for process in fs::read_dir(proc)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
    {
        if process
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
            .is_none()
        {
            continue;
        }
        let path = process.path();
        for fd in fs::read_dir(path.join("fd"))
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
        {
            // Most descriptors are not GPU clients. Avoid reading their fdinfo.
            if !fs::read_link(fd.path()).is_ok_and(|target| target.starts_with("/dev/dri")) {
                continue;
            }
            if let Ok(text) = fs::read_to_string(path.join("fdinfo").join(fd.file_name())) {
                insert(&mut clients, &text);
            }
        }
    }
    clients
}

fn insert(clients: &mut Clients, text: &str) {
    let fields: HashMap<_, _> = text
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key, value.trim()))
        .collect();
    let Some(address) = fields.get("drm-pdev").and_then(|value| pci_address(value)) else {
        return;
    };
    let Some(id) = fields
        .get("drm-client-id")
        .and_then(|value| value.parse::<u64>().ok())
    else {
        return;
    };
    // Client IDs are unique per device, even when inherited or duplicated in another process.
    clients.entry((address, id)).or_insert_with(|| {
        let mut client = Client::default();
        for (key, value) in fields {
            if let Some(engine) = key.strip_prefix("drm-engine-capacity-") {
                if let Ok(capacity) = value.parse::<u64>() {
                    client.capacity.insert(engine.to_owned(), capacity.max(1));
                }
            } else if let Some(engine) = key.strip_prefix("drm-engine-")
                && let Some(time) = value
                    .strip_suffix(" ns")
                    .and_then(|time| time.trim().parse().ok())
            {
                client.engines.insert(engine.to_owned(), time);
            }
        }
        client
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(render: u64, video: u64) -> Clients {
        let mut clients = Clients::new();
        insert(
            &mut clients,
            &format!(
                "drm-pdev: 0000:00:02.0\ndrm-client-id: 24\ndrm-engine-render: {render} ns\ndrm-engine-video: {video} ns\ndrm-engine-capacity-video: 2\n"
            ),
        );
        clients
    }

    #[test]
    fn uses_elapsed_time_and_engine_capacity_after_a_baseline() {
        let mut sampler = Sampler::default();
        let now = Instant::now();
        assert!(sampler.update(snapshot(10, 20), now).is_empty());
        let busy = sampler.update(
            snapshot(100_000_010, 1_200_000_020),
            now + Duration::from_secs(1),
        );
        assert!((busy["0000:00:02.0"] - 60.0).abs() < 0.001);
    }

    #[test]
    fn duplicated_descriptors_are_counted_once_per_device() {
        let text = "drm-pdev: 0000:00:02.0\ndrm-client-id: 1\ndrm-engine-render: 500 ns\n";
        let mut clients = Clients::new();
        insert(&mut clients, text);
        insert(&mut clients, text);
        assert_eq!(clients.len(), 1);
        insert(&mut clients, &text.replace("00:02.0", "01:00.0"));
        assert_eq!(clients.len(), 2);
    }

    #[test]
    fn adds_clients_on_each_engine_without_adding_parallel_engines() {
        let now = Instant::now();
        let mut sampler = Sampler::default();
        let mut first = snapshot(0, 0);
        first.insert(
            ("0000:00:02.0".to_owned(), 25),
            Client {
                engines: HashMap::from([("render".to_owned(), 0)]),
                capacity: HashMap::new(),
            },
        );
        sampler.update(first, now);
        let mut next = snapshot(300_000_000, 1_000_000_000);
        next.insert(
            ("0000:00:02.0".to_owned(), 25),
            Client {
                engines: HashMap::from([("render".to_owned(), 400_000_000)]),
                capacity: HashMap::new(),
            },
        );
        let busy = sampler.update(next, now + Duration::from_secs(1));
        assert!((busy["0000:00:02.0"] - 70.0).abs() < 0.001);
    }

    #[test]
    fn keeps_high_water_mark_and_drops_disappeared_clients() {
        let now = Instant::now();
        let mut sampler = Sampler::default();
        sampler.update(snapshot(500_000_000, 0), now);
        assert!(
            sampler.update(snapshot(400_000_000, 0), now + Duration::from_secs(1))["0000:00:02.0"]
                .abs()
                < f32::EPSILON
        );
        assert!(
            (sampler.update(snapshot(600_000_000, 0), now + Duration::from_secs(2))["0000:00:02.0"]
                - 10.0)
                .abs()
                < 0.001
        );
        assert!(
            sampler
                .update(Clients::new(), now + Duration::from_secs(3))
                .is_empty()
        );
        assert!(
            sampler
                .update(snapshot(9_000_000_000, 0), now + Duration::from_secs(4))
                .is_empty()
        );
    }

    #[test]
    fn ignores_missing_identity_and_unsupported_engine_fields() {
        let mut clients = Clients::new();
        insert(&mut clients, "drm-engine-render: 100 ns");
        assert!(clients.is_empty());
        insert(
            &mut clients,
            "drm-pdev: 0000:00:02.0\ndrm-client-id: 1\ndrm-engine-render: broken\ndrm-engine-copy: 10 ms\n",
        );
        let mut sampler = Sampler::default();
        let now = Instant::now();
        sampler.update(clients, now);
        assert!(
            sampler
                .update(snapshot(100, 200), now + Duration::from_secs(1))
                .is_empty()
        );
    }
}
