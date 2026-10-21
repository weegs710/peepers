//! Lets another process tell the running application what to do, so a compositor key or a script
//! can drive it.

use std::hash::{Hash, Hasher};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use zbus::blocking::{Connection, connection};
use zbus::{fdo, interface};

const NAME: &str = "dev.weegs.Peepers";
const PATH: &str = "/dev/weegs/Peepers";
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
    Quit,
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

/// The object other processes call.
struct Control {
    commands: Sender<Command>,
}

impl Control {
    fn send(&self, command: Command) -> fdo::Result<()> {
        self.commands
            .send(command)
            .map_err(|_| fdo::Error::Failed("peepers is shutting down".to_owned()))
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

    fn quit(&self) -> fdo::Result<()> {
        self.send(Command::Quit)
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
        let bus = connection::Builder::session()
            .and_then(|builder| builder.name(name))
            // The defaults let a second copy take the name from the first.
            .map(|builder| {
                builder
                    .allow_name_replacements(false)
                    .replace_existing_names(false)
            })
            .and_then(|builder| builder.serve_at(PATH, Control { commands: sender }))
            .and_then(connection::Builder::build)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            commands: Commands(Arc::new(Mutex::new(received))),
            _bus: bus,
        })
    }

    #[must_use]
    pub fn commands(&self) -> Commands {
        self.commands.clone()
    }
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
        let error = Proxy::new(&bus, name.as_str(), PATH, NAME)
            .unwrap()
            .call_method("OpenPage", &("nonsense",))
            .unwrap_err();
        let message = reason(&error);
        assert!(message.starts_with("No page called nonsense"));
        assert!(message.contains("disk-space"));

        let commands = listener.commands();
        call(&name, Command::TogglePause).unwrap();
        assert_eq!(commands.recv(), Some(Command::TogglePause));
    }
}
