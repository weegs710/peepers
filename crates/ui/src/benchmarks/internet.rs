use std::fmt;

use iced::widget::{checkbox, column, pick_list, row, text};
use iced::{Element, Fill, Task};
use peepers_core::bench::{Internet, Job, Outcome, Server, servers};

use super::{Benchmarks, Message as Root, Section, Start, figure, not_run, page};
use crate::blocking;
use crate::format;

const INTRO: &str = "Measures your connection against one of the public servers that run the open-source LibreSpeed test. \
It checks the delay and how much it wobbles, then downloads and uploads over one connection and over several, and watches the delay while the line is busy. \
The server it uses sees your address, as every site you visit does, and the test is never part of Run all. It takes about half a minute and moves a few hundred megabytes.";

/// Which server to test against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Nearest,
    Server(Server),
}

impl fmt::Display for Choice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nearest => formatter.write_str("Whichever answers fastest"),
            Self::Server(server) => server.fmt(formatter),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Servers(Result<Vec<Server>, String>),
    Choose(Choice),
    ShowAddress(bool),
}

#[derive(Debug)]
pub struct State {
    choices: Vec<Choice>,
    choice: Choice,
    asked: bool,
    show_address: bool,
    error: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            choices: vec![Choice::Nearest],
            choice: Choice::Nearest,
            asked: false,
            show_address: false,
            error: None,
        }
    }
}

impl State {
    /// Fetches the list of servers the first time the section is opened, so that nothing contacts
    /// the network before it is asked to.
    pub fn load(&self) -> Task<Message> {
        if self.asked {
            return Task::none();
        }
        blocking::run(servers).map(Message::Servers)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Servers(Ok(servers)) => {
                self.asked = true;
                self.error = None;
                self.choices = std::iter::once(Choice::Nearest)
                    .chain(servers.into_iter().map(Choice::Server))
                    .collect();
            }
            Message::Servers(Err(error)) => self.error = Some(error),
            Message::Choose(choice) => self.choice = choice,
            Message::ShowAddress(show) => self.show_address = show,
        }
        Task::none()
    }

    pub fn job(&self) -> Job {
        Job::Internet {
            server: match &self.choice {
                Choice::Nearest => None,
                Choice::Server(server) => Some(server.clone()),
            },
        }
    }
}

pub(super) fn view(app: &Benchmarks) -> Element<'_, Root> {
    let state = &app.internet;
    let mut controls = column![
        row![
            pick_list(state.choices.as_slice(), Some(&state.choice), |choice| {
                Root::Internet(Message::Choose(choice))
            })
            .width(Fill),
            app.run_button("Run", Start::One(Section::Internet)),
        ]
        .spacing(8),
    ]
    .spacing(8);
    if let Some(error) = &state.error {
        controls = controls.push(
            text(format!("The list of servers could not be fetched, so only the automatic choice works: {error}"))
                .size(13)
                .style(text::danger),
        );
    }
    page(
        "Internet",
        INTRO,
        controls.into(),
        app.latest(|outcome| match outcome {
            Outcome::Internet(internet) => Some(internet),
            _ => None,
        })
        .map_or_else(not_run, |internet| results(internet, state.show_address)),
    )
}

fn results(internet: &Internet, show_address: bool) -> Element<'_, Root> {
    let mut figures = column![
        figure(
            "Server",
            internet.server.clone(),
            "The public server this test ran against.",
        ),
        figure(
            "Latency",
            format!("{:.0} ms", internet.latency_ms),
            "How long a tiny request takes to reach the server and come back. Lower is better. It decides how snappy browsing, calls and games feel.",
        ),
        figure(
            "Jitter",
            format!("{:.1} ms", internet.jitter_ms),
            "How much that time wobbles from one request to the next. Lower is steadier: high jitter makes calls and games stutter even when the latency is good.",
        ),
        figure(
            "Packet loss",
            format!("{:.0}%", internet.loss_percent),
            "The share of requests that never came back. Anything above zero on a wired connection is worth chasing.",
        ),
        figure(
            "Download",
            format!(
                "{} over one connection, {} over {} connections",
                format::megabits(internet.download.single),
                format::megabits(internet.download.multi),
                internet.download_streams
            ),
            "One connection shows what a single download or stream gets, and several together show what the line can carry. A big gap usually means distance to the server, not a problem with your line.",
        ),
        figure(
            "Upload",
            format!(
                "{} over one connection, {} over {} connections",
                format::megabits(internet.upload.single),
                format::megabits(internet.upload.multi),
                internet.upload_streams
            ),
            "The same, for sending: video calls, backups and cloud saves.",
        ),
    ]
    .spacing(14);
    if let (Some(down), Some(up)) = (internet.loaded_download_ms, internet.loaded_upload_ms) {
        figures = figures.push(figure(
            "Delay while busy",
            format!(
                "{down:.0} ms while downloading (+{:.0}), {up:.0} ms while uploading (+{:.0})",
                (down - internet.latency_ms).max(0.0),
                (up - internet.latency_ms).max(0.0)
            ),
            queueing(internet.latency_ms, down.max(up)),
        ));
    }
    figures = figures.push(figure(
        "Internet provider",
        internet
            .isp
            .clone()
            .unwrap_or_else(|| "The server did not say".to_owned()),
        "As named by the test server, which often cannot tell.",
    ));
    if internet.address.is_some() || show_address {
        figures = figures.push(address(internet, show_address));
    }
    figures.into()
}

/// The public address, hidden until asked for so a screenshot does not give it away. It tells a
/// website your provider and roughly where you are, though not your name or street.
fn address(internet: &Internet, show: bool) -> Element<'_, Root> {
    let value = match (&internet.address, show) {
        (Some(address), true) => address.clone(),
        (Some(_), false) => "hidden".to_owned(),
        (None, _) => "not kept from earlier runs".to_owned(),
    };
    column![
        figure(
            "Your public address",
            value,
            "The address the server saw you as. Every website sees it, but keep it out of screenshots you post. It is never saved with the history.",
        ),
        checkbox(show)
            .label("Show it")
            .on_toggle(|show| Root::Internet(Message::ShowAddress(show))),
    ]
    .spacing(6)
    .into()
}

/// What the rise in delay under load says, the symptom being calls and games that stutter
/// whenever something transfers.
fn queueing(idle: f32, loaded: f32) -> &'static str {
    match loaded - idle {
        rise if rise < 15.0 => {
            "Barely rises, so a download or upload will not disturb calls and games."
        }
        rise if rise < 50.0 => {
            "Rises a little: busy moments are noticeable in games but fine for calls."
        }
        rise if rise < 150.0 => {
            "Rises a lot (bufferbloat): your router queues traffic, so calls and games will stutter while anything transfers. Smart queue management on the router fixes this."
        }
        _ => {
            "Rises enormously (heavy bufferbloat): everything lags while the line is busy. Smart queue management on the router fixes this."
        }
    }
}

pub(super) fn summary(internet: &Internet) -> String {
    format!(
        "{}: {:.0} ms, jitter {:.1} ms, down {} / {}, up {} / {}",
        internet.server,
        internet.latency_ms,
        internet.jitter_ms,
        format::megabits(internet.download.single),
        format::megabits(internet.download.multi),
        format::megabits(internet.upload.single),
        format::megabits(internet.upload.multi),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_verdict_follows_how_far_the_delay_rises_under_load() {
        assert!(queueing(20.0, 25.0).starts_with("Barely"));
        assert!(queueing(20.0, 50.0).starts_with("Rises a little"));
        assert!(queueing(20.0, 120.0).contains("bufferbloat"));
        assert!(queueing(20.0, 400.0).starts_with("Rises enormously"));
    }

    #[test]
    fn the_server_list_is_not_fetched_twice() {
        let mut state = State::default();
        drop(state.update(Message::Servers(Ok(Vec::new()))));
        assert!(state.asked);
        assert_eq!(state.choices, [Choice::Nearest]);
    }

    #[test]
    fn choosing_nearest_means_no_particular_server() {
        assert_eq!(State::default().job(), Job::Internet { server: None });
    }

    #[test]
    fn a_failed_list_is_reported_and_asked_for_again_next_time() {
        let mut state = State::default();
        drop(state.update(Message::Servers(Err("no route".to_owned()))));
        assert!(!state.asked && state.error.is_some());
    }

    #[test]
    fn the_address_stays_hidden_until_asked_for() {
        let mut state = State::default();
        assert!(!state.show_address);
        drop(state.update(Message::ShowAddress(true)));
        assert!(state.show_address);
    }
}
