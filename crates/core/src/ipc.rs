//! Lets another process tell the running application what to do and read what state it is in, so a
//! compositor key or a script can drive it.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};

use zbus::blocking::{Connection, connection};
use zbus::{fdo, interface};
use zvariant::Value;

const NAME: &str = "dev.weegs.Peepers";
const PATH: &str = "/dev/weegs/Peepers";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const SERVICE_UNKNOWN: &str = "org.freedesktop.DBus.Error.ServiceUnknown";

/// The pages a command can open, spelled as the command line and the bus spell them, in the order
/// the application lists them.
pub const PAGES: [&str; 15] = [
    "processes",
    "performance",
    "pressure",
    "power",
    "flight",
    "benchmarks",
    "connections",
    "drivers",
    "disk-space",
    "apps",
    "info",
    "services",
    "startup",
    "users",
    "settings",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    TogglePopout,
    ShowPopout,
    HidePopout,
    ToggleWindow,
    ShowWindow,
    HideWindow,
    TogglePause,
    Pause,
    Resume,
    StartRecording,
    StopRecording,
    /// Ends the application, asking in its window what to do with a recording that is running.
    Quit,
    QuitSaving,
    QuitDiscarding,
    /// Brings up one of [`PAGES`].
    OpenPage(&'static str),
}

impl Command {
    /// The command that opens the page called `name`, if the application has such a page.
    #[must_use]
    pub fn open_page(name: &str) -> Option<Self> {
        PAGES
            .into_iter()
            .find(|page| *page == name)
            .map(Self::OpenPage)
    }
}

/// What the application says about itself, which other processes can read and watch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent facts published as properties, not the states of one machine"
)]
pub struct State {
    pub paused: bool,
    pub popout_open: bool,
    pub window_open: bool,
    /// One of [`PAGES`], or nothing before the application has said.
    pub page: &'static str,
    pub recording: bool,
}

/// The object other processes call.
struct Control {
    commands: Sender<Command>,
    state: Arc<Mutex<State>>,
}

impl Control {
    fn send(&self, command: Command) -> fdo::Result<()> {
        self.commands
            .send(command)
            .map_err(|_| fdo::Error::Failed("peepers is shutting down".to_owned()))
    }

    fn state(&self) -> State {
        lock(&self.state).clone()
    }
}

#[interface(name = "dev.weegs.Peepers")]
impl Control {
    fn toggle_popout(&self) -> fdo::Result<()> {
        self.send(Command::TogglePopout)
    }

    fn show_popout(&self) -> fdo::Result<()> {
        self.send(Command::ShowPopout)
    }

    fn hide_popout(&self) -> fdo::Result<()> {
        self.send(Command::HidePopout)
    }

    fn toggle_window(&self) -> fdo::Result<()> {
        self.send(Command::ToggleWindow)
    }

    fn show_window(&self) -> fdo::Result<()> {
        self.send(Command::ShowWindow)
    }

    fn hide_window(&self) -> fdo::Result<()> {
        self.send(Command::HideWindow)
    }

    fn toggle_pause(&self) -> fdo::Result<()> {
        self.send(Command::TogglePause)
    }

    fn pause(&self) -> fdo::Result<()> {
        self.send(Command::Pause)
    }

    fn resume(&self) -> fdo::Result<()> {
        self.send(Command::Resume)
    }

    fn start_recording(&self) -> fdo::Result<()> {
        self.send(Command::StartRecording)
    }

    fn stop_recording(&self) -> fdo::Result<()> {
        self.send(Command::StopRecording)
    }

    /// Refused while a recording runs, because the application then waits for an answer in its
    /// window, and a caller that cannot see the window needs to hear that it did not end.
    fn quit(&self) -> fdo::Result<()> {
        self.send(Command::Quit)?;
        if self.state().recording {
            return Err(fdo::Error::Failed(
                "A recording is running, so peepers is asking in its window. \
                 Quit again saying save or discard to answer it."
                    .to_owned(),
            ));
        }
        Ok(())
    }

    fn quit_saving(&self) -> fdo::Result<()> {
        self.send(Command::QuitSaving)
    }

    fn quit_discarding(&self) -> fdo::Result<()> {
        self.send(Command::QuitDiscarding)
    }

    fn open_page(&self, page: &str) -> fdo::Result<()> {
        let command = Command::open_page(page).ok_or_else(|| {
            fdo::Error::InvalidArgs(format!(
                "No page called {page}. Pages: {}",
                PAGES.join(", ")
            ))
        })?;
        self.send(command)
    }

    #[zbus(property)]
    fn paused(&self) -> bool {
        self.state().paused
    }

    #[zbus(property)]
    fn popout_open(&self) -> bool {
        self.state().popout_open
    }

    #[zbus(property)]
    fn window_open(&self) -> bool {
        self.state().window_open
    }

    #[zbus(property)]
    fn page(&self) -> String {
        self.state().page.to_owned()
    }

    #[zbus(property)]
    fn recording(&self) -> bool {
        self.state().recording
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The commands that reach the application, which a subscription can wait on and tell apart from
/// another listener's by identity.
#[derive(Debug, Clone)]
pub struct Commands(Arc<Mutex<Receiver<Command>>>);

impl Commands {
    /// Waits for the next command, or returns `None` once the listener is gone.
    #[must_use]
    pub fn recv(&self) -> Option<Command> {
        lock(&self.0).recv().ok()
    }
}

impl Hash for Commands {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

/// The application's end: it owns the bus name for as long as it lives.
#[derive(Debug)]
pub struct Listener {
    commands: Commands,
    /// Handed to the thread that announces what changed, so a slow bus never holds the caller up.
    published: Option<Sender<State>>,
    announcer: Option<JoinHandle<()>>,
    _bus: Connection,
}

impl Listener {
    /// # Errors
    ///
    /// Fails when there is no session bus or another copy already owns the name.
    pub fn start() -> Result<Self, String> {
        Self::claim(NAME)
    }

    fn claim(name: &str) -> Result<Self, String> {
        let (sender, received) = mpsc::channel();
        let state = Arc::new(Mutex::new(State::default()));
        let bus = connection::Builder::session()
            .and_then(|builder| builder.name(name))
            // The defaults let a second copy take the name from the first.
            .map(|builder| {
                builder
                    .allow_name_replacements(false)
                    .replace_existing_names(false)
            })
            .and_then(|builder| {
                builder.serve_at(
                    PATH,
                    Control {
                        commands: sender,
                        state: Arc::clone(&state),
                    },
                )
            })
            .and_then(connection::Builder::build)
            .map_err(|error| error.to_string())?;
        let (published, states) = mpsc::channel();
        let announcer = {
            let bus = bus.clone();
            thread::spawn(move || announce(&bus, &state, &states))
        };
        Ok(Self {
            commands: Commands(Arc::new(Mutex::new(received))),
            published: Some(published),
            announcer: Some(announcer),
            _bus: bus,
        })
    }

    #[must_use]
    pub fn commands(&self) -> Commands {
        self.commands.clone()
    }

    /// Makes `state` what other processes read, and tells those watching which parts changed.
    pub fn publish(&self, state: State) {
        if let Some(published) = &self.published {
            let _ = published.send(state);
        }
    }
}

impl Drop for Listener {
    /// Waits for the announcer so the bus name is released by the time this returns.
    fn drop(&mut self) {
        drop(self.published.take());
        if let Some(announcer) = self.announcer.take() {
            let _ = announcer.join();
        }
    }
}

fn announce(bus: &Connection, shared: &Mutex<State>, states: &Receiver<State>) {
    while let Ok(next) = states.recv() {
        let before = std::mem::replace(&mut *lock(shared), next.clone());
        let changed = changes(&before, &next);
        if !changed.is_empty() {
            let _ = bus.emit_signal(
                None::<&str>,
                PATH,
                PROPERTIES,
                "PropertiesChanged",
                &(NAME, changed, Vec::<&str>::new()),
            );
        }
    }
}

/// The properties whose value differs between two states, by their bus names.
fn changes(before: &State, after: &State) -> HashMap<&'static str, Value<'static>> {
    let mut changed = HashMap::new();
    if before.paused != after.paused {
        changed.insert("Paused", Value::from(after.paused));
    }
    if before.popout_open != after.popout_open {
        changed.insert("PopoutOpen", Value::from(after.popout_open));
    }
    if before.window_open != after.window_open {
        changed.insert("WindowOpen", Value::from(after.window_open));
    }
    if before.page != after.page {
        changed.insert("Page", Value::from(after.page));
    }
    if before.recording != after.recording {
        changed.insert("Recording", Value::from(after.recording));
    }
    changed
}

/// Asks the running application to carry out `command`.
///
/// # Errors
///
/// Fails when there is no session bus, no application is listening, or it refuses the command.
pub fn send(command: Command) -> Result<(), String> {
    call(NAME, command)
}

fn call(name: &str, command: Command) -> Result<(), String> {
    let (method, page) = match command {
        Command::TogglePopout => ("TogglePopout", None),
        Command::ShowPopout => ("ShowPopout", None),
        Command::HidePopout => ("HidePopout", None),
        Command::ToggleWindow => ("ToggleWindow", None),
        Command::ShowWindow => ("ShowWindow", None),
        Command::HideWindow => ("HideWindow", None),
        Command::TogglePause => ("TogglePause", None),
        Command::Pause => ("Pause", None),
        Command::Resume => ("Resume", None),
        Command::StartRecording => ("StartRecording", None),
        Command::StopRecording => ("StopRecording", None),
        Command::Quit => ("Quit", None),
        Command::QuitSaving => ("QuitSaving", None),
        Command::QuitDiscarding => ("QuitDiscarding", None),
        Command::OpenPage(page) => ("OpenPage", Some(page)),
    };
    let bus = Connection::session().map_err(|error| reason(&error))?;
    match page {
        Some(page) => bus.call_method(Some(name), PATH, Some(NAME), method, &(page,)),
        None => bus.call_method(Some(name), PATH, Some(NAME), method, &()),
    }
    .map(drop)
    .map_err(|error| reason(&error))
}

/// What to tell the user about a failed call: the application's own words when it answered, and
/// that it could not be reached otherwise.
fn reason(error: &zbus::Error) -> String {
    match error {
        zbus::Error::MethodError(name, Some(message), _) if name.as_str() != SERVICE_UNKNOWN => {
            message.clone()
        }
        other => format!("Could not reach a running peepers: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use zbus::blocking::Proxy;

    use super::*;

    /// A name of its own, so a peepers running on the same bus does not answer the test.
    fn test_name(test: &str) -> String {
        format!("dev.weegs.PeepersTest{}{test}", std::process::id())
    }

    fn proxy<'a>(bus: &'a Connection, name: &'a str, interface: &'a str) -> Proxy<'a> {
        Proxy::new(bus, name, PATH, interface).unwrap()
    }

    #[test]
    fn a_page_is_found_by_its_name_and_only_by_it() {
        assert_eq!(
            Command::open_page("services"),
            Some(Command::OpenPage("services"))
        );
        assert_eq!(Command::open_page("Services"), None);
        assert_eq!(Command::open_page(""), None);
    }

    #[test]
    fn only_what_differs_is_announced() {
        let before = State::default();
        assert!(changes(&before, &before).is_empty());
        let after = State {
            paused: true,
            page: "power",
            ..State::default()
        };
        let mut names: Vec<_> = changes(&before, &after).into_keys().collect();
        names.sort_unstable();
        assert_eq!(names, ["Page", "Paused"]);
    }

    #[test]
    #[ignore = "needs a session bus"]
    fn a_command_reaches_the_listener_that_owns_the_name_and_only_one_can() {
        let name = test_name("Owner");
        let listener = Listener::claim(&name).unwrap();
        assert!(Listener::claim(&name).is_err());

        let commands = listener.commands();
        for command in [
            Command::TogglePopout,
            Command::ShowPopout,
            Command::HidePopout,
            Command::ToggleWindow,
            Command::ShowWindow,
            Command::HideWindow,
            Command::TogglePause,
            Command::Pause,
            Command::Resume,
            Command::StartRecording,
            Command::StopRecording,
            Command::Quit,
            Command::QuitSaving,
            Command::QuitDiscarding,
            Command::OpenPage("flight"),
        ] {
            call(&name, command).unwrap();
            assert_eq!(commands.recv(), Some(command));
        }

        drop(listener);
        let error = call(&name, Command::TogglePopout).unwrap_err();
        assert!(error.starts_with("Could not reach a running peepers"));
    }

    #[test]
    #[ignore = "needs a session bus"]
    fn a_page_that_does_not_exist_is_refused_with_the_pages_that_do() {
        let name = test_name("Refuse");
        let listener = Listener::claim(&name).unwrap();
        let bus = Connection::session().unwrap();
        let error = proxy(&bus, &name, NAME)
            .call_method("OpenPage", &("nonsense",))
            .unwrap_err();
        let message = reason(&error);
        assert!(message.starts_with("No page called nonsense"));
        assert!(message.contains("disk-space"));

        let commands = listener.commands();
        call(&name, Command::TogglePause).unwrap();
        assert_eq!(commands.recv(), Some(Command::TogglePause));
    }

    #[test]
    #[ignore = "needs a session bus"]
    fn quitting_is_refused_while_a_recording_runs_but_the_application_still_hears_it() {
        let name = test_name("Quit");
        let listener = Listener::claim(&name).unwrap();
        let commands = listener.commands();
        call(&name, Command::Quit).unwrap();
        assert_eq!(commands.recv(), Some(Command::Quit));

        listener.publish(State {
            recording: true,
            ..State::default()
        });
        let bus = Connection::session().unwrap();
        let properties = proxy(&bus, &name, NAME);
        for _ in 0..500 {
            if properties.get_property::<bool>("Recording").unwrap() {
                break;
            }
            thread::sleep(std::time::Duration::from_millis(10));
        }
        let error = call(&name, Command::Quit).unwrap_err();
        assert!(error.contains("A recording is running"));
        assert_eq!(commands.recv(), Some(Command::Quit));

        call(&name, Command::QuitSaving).unwrap();
        assert_eq!(commands.recv(), Some(Command::QuitSaving));
        call(&name, Command::QuitDiscarding).unwrap();
        assert_eq!(commands.recv(), Some(Command::QuitDiscarding));
    }

    #[test]
    #[ignore = "needs a session bus"]
    fn the_state_can_be_read_and_a_change_is_announced() {
        let name = test_name("State");
        let listener = Listener::claim(&name).unwrap();
        let bus = Connection::session().unwrap();
        let properties = proxy(&bus, &name, NAME);
        assert!(!properties.get_property::<bool>("Paused").unwrap());

        let watch = proxy(&bus, &name, PROPERTIES);
        let signals = watch.receive_signal("PropertiesChanged").unwrap();
        let (heard, arrived) = mpsc::channel();
        thread::spawn(move || {
            for signal in signals {
                let body = signal.body();
                let (_, changed, _): (String, HashMap<String, zvariant::OwnedValue>, Vec<String>) =
                    body.deserialize().unwrap();
                let _ = heard.send(changed);
            }
        });

        listener.publish(State {
            paused: true,
            page: "power",
            ..State::default()
        });
        let changed = arrived
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let mut names: Vec<_> = changed.keys().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(names, ["Page", "Paused"]);
        assert!(properties.get_property::<bool>("Paused").unwrap());
        assert_eq!(properties.get_property::<String>("Page").unwrap(), "power");
    }
}
