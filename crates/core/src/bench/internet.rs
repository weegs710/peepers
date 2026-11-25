use std::fmt;
use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use ureq::{Agent, SendBody};

use super::{Pair, check};
use crate::math::per_second;

/// The public list of servers that run the open-source `LibreSpeed` backend.
const LIST_URL: &str = "https://librespeed.org/backend-servers/servers.php";
const PING_TIMEOUT: Duration = Duration::from_secs(3);
/// Long enough for the one upload that checks a server takes the test's requests, on a slow line.
const CHECK_TIMEOUT: Duration = Duration::from_secs(20);
/// How many of the nearest servers are tried before giving up on the automatic choice.
const ATTEMPTS: usize = 5;
/// Pings that pick the nearest servers, and the pings that measure latency and jitter.
const PROBES: usize = 3;
const PINGS: usize = 20;
const PING_GAP: Duration = Duration::from_millis(50);
const LOADED_PING_GAP: Duration = Duration::from_millis(200);
/// Parallel connections in the multi-connection phases, as the `LibreSpeed` client uses.
const DOWNLOAD_STREAMS: usize = 6;
const UPLOAD_STREAMS: usize = 3;
/// How much one download request asks the server for, in mebibytes.
const DOWNLOAD_REQUEST_MIB: u32 = 20;
const UPLOAD_REQUEST: usize = 4 << 20;
const READ_BLOCK: usize = 64 << 10;
/// Gives each request a URL no cache has seen.
static NONCE: AtomicU64 = AtomicU64::new(0);

/// How long each transfer phase waits for the connections to speed up before it measures, and
/// how long it measures.
#[derive(Debug, Clone, Copy)]
struct Plan {
    download_grace: Duration,
    upload_grace: Duration,
    measure: Duration,
    /// Streams start this far apart, so that they do not all ramp up in step.
    stagger: Duration,
}

const PLAN: Plan = Plan {
    download_grace: Duration::from_millis(1500),
    upload_grace: Duration::from_secs(3),
    measure: Duration::from_secs(5),
    stagger: Duration::from_millis(300),
};

/// A public server, with the addresses its parts answer on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Server {
    pub id: u32,
    pub name: String,
    download: String,
    upload: String,
    ping: String,
    address: String,
}

impl fmt::Display for Server {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.name)
    }
}

/// One entry of the public list.
#[derive(Deserialize)]
struct Listed {
    id: u32,
    name: String,
    server: String,
    #[serde(rename = "dlURL")]
    download: String,
    #[serde(rename = "ulURL")]
    upload: String,
    #[serde(rename = "pingURL")]
    ping: String,
    #[serde(rename = "getIpURL")]
    address: String,
}

impl From<Listed> for Server {
    fn from(listed: Listed) -> Self {
        let at = |path: &str| format!("{}/{path}", listed.server.trim_end_matches('/'));
        Self {
            id: listed.id,
            download: at(&listed.download),
            upload: at(&listed.upload),
            ping: at(&listed.ping),
            address: at(&listed.address),
            name: listed.name,
        }
    }
}

/// Latency and jitter in milliseconds, packet loss in percent, and speeds in bytes per second
/// over one connection and over several at once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Internet {
    pub server: String,
    /// The typical round trip to the server with nothing else on the line.
    pub latency_ms: f32,
    /// How much a round trip differs from the one before on average.
    pub jitter_ms: f32,
    /// The share of idle pings that never came back.
    pub loss_percent: f32,
    pub download: Pair<u64>,
    pub upload: Pair<u64>,
    pub download_streams: usize,
    pub upload_streams: usize,
    /// The round trip while the line was saturated by the multi-connection download and upload.
    /// A big rise over `latency_ms` is bufferbloat: the router queues traffic instead of
    /// dropping it.
    pub loaded_download_ms: Option<f32>,
    pub loaded_upload_ms: Option<f32>,
    /// The ISP the server named for this connection, if it knows.
    pub isp: Option<String>,
    /// The public address the server saw. Left out of saved results, so that sharing the history
    /// does not share it.
    #[serde(skip)]
    pub address: Option<String>,
}

/// Redirects are not followed, so a listed server cannot send this machine's requests to another
/// host, such as one on the local network.
fn agent(timeout: Option<Duration>) -> Agent {
    Agent::new_with_config(
        Agent::config_builder()
            .timeout_global(timeout)
            .max_redirects(0)
            .build(),
    )
}

/// The servers on the public list, nearest to everyone unsorted.
///
/// # Errors
///
/// Fails when the list cannot be fetched or is not what it should be.
pub fn servers() -> Result<Vec<Server>, String> {
    let text = agent(None)
        .get(LIST_URL)
        .call()
        .map_err(|error| format!("The list of servers {}", describe(error)))?
        .into_body()
        .read_to_string()
        .map_err(|error| error.to_string())?;
    from_list(&text)
}

/// The listed servers reached over https, so that no one on the path can answer for one.
fn from_list(text: &str) -> Result<Vec<Server>, String> {
    let listed: Vec<Listed> = serde_json::from_str(text).map_err(|error| error.to_string())?;
    Ok(listed
        .into_iter()
        .filter(|listed| listed.server.starts_with("https://"))
        .map(Server::from)
        .collect())
}

/// Measures against `server`, or against whichever of the public servers answers fastest.
pub(super) fn run(
    server: Option<&Server>,
    stage: &dyn Fn(&'static str),
    stop: &AtomicBool,
) -> Result<Internet, String> {
    measure(server, stage, stop, &PLAN)
}

fn measure(
    chosen: Option<&Server>,
    stage: &dyn Fn(&'static str),
    stop: &AtomicBool,
    plan: &Plan,
) -> Result<Internet, String> {
    let server = if let Some(server) = chosen {
        stage("Checking the server");
        usable(server)?;
        server.clone()
    } else {
        stage("Finding the nearest server");
        let ranked = nearest(&servers()?, stop)?;
        stage("Checking the server");
        first_usable(&ranked, stop)?
    };
    let agent = agent(Some(PING_TIMEOUT));
    let (address, isp) = who_am_i(&agent, &server);

    stage("Measuring latency and jitter");
    let pings = pings(&agent, &server.ping, PINGS, PING_GAP, stop)?;
    let answered: Vec<f32> = pings.iter().flatten().copied().collect();
    if answered.is_empty() {
        return Err(format!("{} did not answer any ping", server.name));
    }
    let latency_ms = median(&mut answered.clone());
    let (jitter_ms, loss_percent) = (jitter(&answered), loss(&pings));

    let transfers = Transfers {
        server: &server,
        plan,
        stop,
    };
    stage("Download over one connection");
    let download_single = transfers.download(1, false)?;
    stage("Download over several connections");
    let download_multi = transfers.download(DOWNLOAD_STREAMS, true)?;
    stage("Upload over one connection");
    let upload_single = transfers.upload(1, false)?;
    stage("Upload over several connections");
    let upload_multi = transfers.upload(UPLOAD_STREAMS, true)?;

    Ok(Internet {
        server: server.name,
        latency_ms,
        jitter_ms,
        loss_percent,
        download: Pair {
            single: download_single.rate,
            multi: download_multi.rate,
        },
        upload: Pair {
            single: upload_single.rate,
            multi: upload_multi.rate,
        },
        download_streams: DOWNLOAD_STREAMS,
        upload_streams: UPLOAD_STREAMS,
        loaded_download_ms: download_multi.loaded_ms,
        loaded_upload_ms: upload_multi.loaded_ms,
        isp,
        address,
    })
}

fn describe(error: ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(413) => {
            "refused a request of this size (HTTP 413, payload too large)".to_owned()
        }
        ureq::Error::StatusCode(429) => "is limiting this address for now (HTTP 429)".to_owned(),
        ureq::Error::StatusCode(code) => format!("answered HTTP {code}"),
        other => other.to_string(),
    }
}

/// Whether the server takes the requests the test makes, found out by making one of each, so
/// that a server which caps upload sizes or is down is passed over before anything is measured.
fn usable(server: &Server) -> Result<(), String> {
    let agent = agent(Some(CHECK_TIMEOUT));
    let upload = SendBody::from_owned_reader(io::repeat(0).take(UPLOAD_REQUEST as u64));
    let uploaded = agent
        .post(format!("{}?r={}", server.upload, nonce()))
        .send(upload)
        .map_err(|error| format!("{} {} for an upload", server.name, describe(error)))?;
    not_moved(server, uploaded.status(), "an upload")?;
    let downloaded = agent
        .get(format!("{}?ckSize=1&r={}", server.download, nonce()))
        .call()
        .map_err(|error| format!("{} {} for a download", server.name, describe(error)))?;
    not_moved(server, downloaded.status(), "a download")?;
    let mut body = downloaded.into_body().into_reader();
    io::copy(&mut body, &mut io::sink())
        .map(drop)
        .map_err(|error| format!("{}: {error}", server.name))
}

/// A redirect is refused here rather than followed, so that a server which moved is passed over.
fn not_moved(server: &Server, status: ureq::http::StatusCode, request: &str) -> Result<(), String> {
    if status.is_redirection() {
        return Err(format!(
            "{} answered HTTP {} for {request}, a redirect, which is not followed",
            server.name,
            status.as_u16()
        ));
    }
    Ok(())
}

/// The first of the nearest servers that takes the test's requests.
fn first_usable(ranked: &[Server], stop: &AtomicBool) -> Result<Server, String> {
    let mut reasons = Vec::new();
    for server in ranked.iter().take(ATTEMPTS) {
        check(stop)?;
        match usable(server) {
            Ok(()) => return Ok(server.clone()),
            Err(reason) => reasons.push(reason),
        }
    }
    Err(format!(
        "None of the nearest servers could be used: {}",
        reasons.join("; ")
    ))
}

fn nonce() -> u64 {
    NONCE.fetch_add(1, Ordering::Relaxed)
}

/// One round trip to `url` in milliseconds, or `None` when it fails or takes too long.
fn ping(agent: &Agent, url: &str) -> Option<f32> {
    let began = Instant::now();
    let mut body = agent
        .get(format!("{url}?r={}", nonce()))
        .call()
        .ok()?
        .into_body()
        .into_reader();
    io::copy(&mut body, &mut io::sink()).ok()?;
    Some(began.elapsed().as_secs_f32() * 1000.0)
}

/// `count` pings in the order they were made, with `gap` between them. The first request also
/// pays for the connection handshake, so it is made and not counted.
fn pings(
    agent: &Agent,
    url: &str,
    count: usize,
    gap: Duration,
    stop: &AtomicBool,
) -> Result<Vec<Option<f32>>, String> {
    ping(agent, url);
    let mut results = Vec::with_capacity(count);
    for _ in 0..count {
        check(stop)?;
        results.push(ping(agent, url));
        thread::sleep(gap);
    }
    Ok(results)
}

/// The servers that answer, soonest first, found by asking every server at once.
fn nearest(servers: &[Server], stop: &AtomicBool) -> Result<Vec<Server>, String> {
    let agent = agent(Some(PING_TIMEOUT));
    let times: Vec<Option<f32>> = thread::scope(|scope| {
        let probes: Vec<_> = servers
            .iter()
            .map(|server| {
                scope.spawn(|| {
                    let mut answered: Vec<f32> = (0..PROBES)
                        .filter_map(|_| ping(&agent, &server.ping))
                        .collect();
                    (!answered.is_empty()).then(|| median(&mut answered))
                })
            })
            .collect();
        probes
            .into_iter()
            .map(|probe| probe.join().unwrap_or(None))
            .collect()
    });
    check(stop)?;
    let mut answered: Vec<_> = servers
        .iter()
        .zip(times)
        .filter_map(|(server, time)| Some((server, time?)))
        .collect();
    answered.sort_by(|a, b| a.1.total_cmp(&b.1));
    if answered.is_empty() {
        return Err("None of the public servers answered. Is the network up?".to_owned());
    }
    Ok(answered
        .into_iter()
        .map(|(server, _)| server.clone())
        .collect())
}

/// The address the server saw and the ISP it names for it. A server that cannot look an ISP up
/// says so, and a failure to ask at all is not worth failing the benchmark for.
fn who_am_i(agent: &Agent, server: &Server) -> (Option<String>, Option<String>) {
    let reply = agent
        .get(format!("{}?isp=true&distance=km", server.address))
        .call()
        .ok()
        .and_then(|response| response.into_body().read_to_string().ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    reply
        .as_ref()
        .and_then(|value| value["processedString"].as_str())
        .map_or((None, None), parse_identity)
}

/// `address - ISP, Country (12 km)`, or a plain description such as `private IPv4 access`.
fn parse_identity(text: &str) -> (Option<String>, Option<String>) {
    let Some((address, rest)) = text.split_once(" - ") else {
        return (None, None);
    };
    let isp = rest
        .rsplit_once(" (")
        .map_or(rest, |(isp, _)| isp)
        .trim()
        .to_owned();
    (
        Some(address.trim().to_owned()),
        (isp != "Unknown ISP" && !isp.is_empty()).then_some(isp),
    )
}

fn median(values: &mut [f32]) -> f32 {
    values.sort_by(f32::total_cmp);
    values.get(values.len() / 2).copied().unwrap_or(0.0)
}

/// How much one round trip differs from the one before on average, which is what a call or a
/// game feels as stutter even when the median is low.
#[expect(
    clippy::cast_precision_loss,
    reason = "a count of round trips is far below 2^23"
)]
fn jitter(round_trips: &[f32]) -> f32 {
    let steps = round_trips.len().saturating_sub(1);
    if steps == 0 {
        return 0.0;
    }
    let total: f32 = round_trips
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .sum();
    total / steps as f32
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a count of pings is far below 2^23"
)]
fn loss(pings: &[Option<f32>]) -> f32 {
    if pings.is_empty() {
        return 0.0;
    }
    100.0 * pings.iter().filter(|ping| ping.is_none()).count() as f32 / pings.len() as f32
}

struct Transfer {
    rate: u64,
    loaded_ms: Option<f32>,
}

struct Transfers<'a> {
    server: &'a Server,
    plan: &'a Plan,
    stop: &'a AtomicBool,
}

impl Transfers<'_> {
    fn failed(&self, error: ureq::Error) -> String {
        format!("{} {}", self.server.name, describe(error))
    }

    fn download(&self, streams: usize, loaded: bool) -> Result<Transfer, String> {
        let agent = agent(None);
        let url = format!("{}?ckSize={DOWNLOAD_REQUEST_MIB}", self.server.download);
        self.run(
            streams,
            self.plan.download_grace,
            loaded,
            |received, deadline| {
                let mut block = vec![0_u8; READ_BLOCK];
                while Instant::now() < deadline {
                    check(self.stop)?;
                    let mut body = agent
                        .get(format!("{url}&r={}", nonce()))
                        .call()
                        .map_err(|error| self.failed(error))?
                        .into_body()
                        .into_reader();
                    while Instant::now() < deadline {
                        match body.read(&mut block).map_err(|error| error.to_string())? {
                            0 => break,
                            count => received.fetch_add(count as u64, Ordering::Relaxed),
                        };
                    }
                }
                Ok(())
            },
        )
    }

    fn upload(&self, streams: usize, loaded: bool) -> Result<Transfer, String> {
        let agent = agent(None);
        self.run(streams, self.plan.upload_grace, loaded, |sent, deadline| {
            while Instant::now() < deadline {
                check(self.stop)?;
                let mut payload = Payload {
                    left: UPLOAD_REQUEST,
                    sent,
                    deadline,
                    stop: self.stop,
                };
                let result = agent
                    .post(format!("{}?r={}", self.server.upload, nonce()))
                    .send(SendBody::from_reader(&mut payload));
                if let Err(error) = result {
                    check(self.stop)?;
                    return Err(self.failed(error));
                }
            }
            Ok(())
        })
    }

    /// Runs `streams` workers against `counter`, waits out the grace time, and returns the rate
    /// they reach over the measuring time that follows. With `loaded`, the server is also pinged
    /// over a connection of its own while the line is busy.
    fn run(
        &self,
        streams: usize,
        grace: Duration,
        loaded: bool,
        worker: impl Fn(&AtomicU64, Instant) -> Result<(), String> + Sync,
    ) -> Result<Transfer, String> {
        let counter = AtomicU64::new(0);
        let start = Instant::now();
        let (measure_from, deadline) = (start + grace, start + grace + self.plan.measure);
        thread::scope(|scope| {
            let workers: Vec<_> = (0..streams)
                .map(|index| {
                    let (counter, worker) = (&counter, &worker);
                    scope.spawn(move || {
                        thread::sleep(self.plan.stagger * u32::try_from(index).unwrap_or(u32::MAX));
                        worker(counter, deadline)
                    })
                })
                .collect();
            let pinger = loaded.then(|| {
                scope.spawn(|| {
                    wait_until(measure_from, self.stop)?;
                    Ok::<_, String>(self.loaded_pings(deadline))
                })
            });

            let measured = wait_until(measure_from, self.stop).and_then(|()| {
                let (before, from) = (counter.load(Ordering::Relaxed), Instant::now());
                wait_until(deadline, self.stop)?;
                let after = counter.load(Ordering::Relaxed);
                Ok(per_second(after.saturating_sub(before), from.elapsed()))
            });
            for worker in workers {
                worker
                    .join()
                    .unwrap_or_else(|_| Err("A transfer stream panicked".to_owned()))?;
            }
            let loaded_ms = match pinger {
                Some(pinger) => pinger
                    .join()
                    .unwrap_or(Err("The pinger panicked".to_owned()))?,
                None => None,
            };
            Ok(Transfer {
                rate: measured?,
                loaded_ms,
            })
        })
    }

    /// The median round trip, from a connection of its own, until `deadline`.
    fn loaded_pings(&self, deadline: Instant) -> Option<f32> {
        let agent = agent(Some(PING_TIMEOUT));
        let mut answered = Vec::new();
        while Instant::now() < deadline && check(self.stop).is_ok() {
            answered.extend(ping(&agent, &self.server.ping));
            thread::sleep(LOADED_PING_GAP);
        }
        (!answered.is_empty()).then(|| median(&mut answered))
    }
}

/// Sleeps until `moment`, a little at a time so that a stop is noticed.
fn wait_until(moment: Instant, stop: &AtomicBool) -> Result<(), String> {
    loop {
        check(stop)?;
        let left = moment.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(());
        }
        thread::sleep(left.min(Duration::from_millis(100)));
    }
}

/// Zeros for an upload request, counted as the connection takes them. The body ends early once
/// the measuring time is over or the run is stopped, which for a body of unknown length is a
/// clean end.
struct Payload<'a> {
    left: usize,
    sent: &'a AtomicU64,
    deadline: Instant,
    stop: &'a AtomicBool,
}

impl Read for Payload<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.left == 0 || Instant::now() >= self.deadline || self.stop.load(Ordering::Relaxed) {
            return Ok(0);
        }
        let count = buffer.len().min(self.left).min(READ_BLOCK);
        buffer[..count].fill(0);
        self.left -= count;
        self.sent.fetch_add(count as u64, Ordering::Relaxed);
        Ok(count)
    }
}

#[cfg(test)]
mod tests;
