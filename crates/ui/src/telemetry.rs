use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use iced::futures::channel::mpsc::Sender;
use iced::{Subscription, stream};
use peepers_core::telemetry::Sampler;

use crate::Message;

/// Samples on a thread of its own so a slow reading never stalls the interface, once per
/// `interval` however long a sample takes.
pub fn subscription(interval: Duration) -> Subscription<Message> {
    Subscription::run_with(interval, |&interval| {
        stream::channel(1, async move |output| {
            thread::spawn(move || sample_forever(output, interval));
        })
    })
}

fn sample_forever(mut output: Sender<Message>, interval: Duration) {
    let mut sampler = Sampler::default();
    loop {
        let started = Instant::now();
        let message = match sampler.sample() {
            Ok(sample) => Message::Sampled(Arc::new(sample)),
            Err(error) => Message::Failed(error.kind()),
        };
        if let Err(error) = output.try_send(message)
            && error.is_disconnected()
        {
            return;
        }
        thread::sleep(interval.saturating_sub(started.elapsed()));
    }
}
