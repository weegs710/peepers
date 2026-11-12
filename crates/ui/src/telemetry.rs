use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use iced::futures::channel::mpsc::Sender;
use iced::{Subscription, stream};
use peepers_core::energy::Collector;
use peepers_core::telemetry::Sampler;

use crate::Message;

/// Stable subscription identity: cadence and range changes preserve counter baselines and storage.
#[derive(Clone)]
pub struct Control(Arc<Shared>);

struct Shared {
    millis: AtomicU64,
    hours: AtomicU64,
    flush: Mutex<Vec<mpsc::Sender<Result<(), String>>>>,
}

impl Default for Control {
    fn default() -> Self {
        Self(Arc::new(Shared {
            millis: AtomicU64::new(1000),
            hours: AtomicU64::new(24),
            flush: Mutex::new(Vec::new()),
        }))
    }
}
impl PartialEq for Control {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for Control {}
impl Hash for Control {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

impl Control {
    pub fn range(&self, hours: u64) {
        self.0.hours.store(hours, Ordering::Relaxed);
    }

    pub fn flush(&self) -> Result<(), String> {
        let (send, receive) = mpsc::channel();
        self.0.flush.lock().map_err(|e| e.to_string())?.push(send);
        receive
            .recv_timeout(Duration::from_secs(15))
            .map_err(|_| "Energy collector did not finish saving within 15 seconds".to_owned())?
    }
}

pub fn subscription(control: &Control, interval: Duration) -> Subscription<Message> {
    control.0.millis.store(
        u64::try_from(interval.as_millis())
            .unwrap_or(1000)
            .clamp(100, 5000),
        Ordering::Relaxed,
    );
    Subscription::run_with(control.clone(), |control| {
        let control = control.clone();
        stream::channel(4, async move |output| {
            thread::spawn(move || sample_forever(output, &control));
        })
    })
}

fn sample_forever(mut output: Sender<Message>, control: &Control) {
    let mut sampler = Sampler::default();
    let mut energy = Collector::open();
    let mut last_report = None;
    let mut range = 0;
    loop {
        let started = Instant::now();
        let message = match sampler.sample() {
            Ok(sample) => {
                energy.record(&sample, Instant::now(), SystemTime::now());
                Message::Sampled(Arc::new(sample))
            }
            Err(error) => {
                energy.gap();
                Message::Failed(error.kind())
            }
        };
        let hours = control.0.hours.load(Ordering::Relaxed);
        if range != hours
            || last_report.is_none_or(|at: Instant| at.elapsed() >= Duration::from_secs(10))
        {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            if output
                .try_send(Message::Energy(crate::energy::Message::Reported(Arc::new(
                    energy.report(hours, now),
                ))))
                .is_ok()
            {
                last_report = Some(Instant::now());
                range = hours;
            }
        }
        if let Err(error) = output.try_send(message)
            && error.is_disconnected()
        {
            let _ = energy.flush();
            return;
        }
        if let Ok(mut requests) = control.0.flush.lock()
            && !requests.is_empty()
        {
            let result = energy.flush();
            for request in requests.drain(..) {
                let _ = request.send(result.clone());
            }
        }
        let interval = Duration::from_millis(control.0.millis.load(Ordering::Relaxed));
        thread::sleep(interval.saturating_sub(started.elapsed()));
    }
}
