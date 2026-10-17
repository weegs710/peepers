//! Lets another process tell the running application what to do, so a compositor key can ask it
//! for the popout.

use std::hash::{Hash, Hasher};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};

use zbus::blocking::{Connection, connection};
use zbus::interface;

const NAME: &str = "dev.weegs.Peepers";
const PATH: &str = "/dev/weegs/Peepers";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    TogglePopout,
}

/// The object other processes call.
struct Control {
    commands: Sender<Command>,
}

#[interface(name = "dev.weegs.Peepers")]
impl Control {
    fn toggle_popout(&self) {
        let _ = self.commands.send(Command::TogglePopout);
    }
}

/// The commands that reach the application, which a subscription can wait on and tell apart from
/// another listener's by identity.
#[derive(Debug, Clone)]
pub struct Commands(Arc<Mutex<Receiver<Command>>>);

impl Commands {
    /// Waits for the next command, or returns `None` once the listener is gone.
    #[must_use]
    pub fn recv(&self) -> Option<Command> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .recv()
            .ok()
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
/// Fails when there is no session bus or no application is listening.
pub fn send(command: Command) -> Result<(), String> {
    call(NAME, command)
}

fn call(name: &str, command: Command) -> Result<(), String> {
    let method = match command {
        Command::TogglePopout => "TogglePopout",
    };
    Connection::session()
        .and_then(|bus| bus.call_method(Some(name), PATH, Some(NAME), method, &()))
        .map(drop)
        .map_err(|error| format!("Could not reach a running peepers: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name of its own, so a peepers running on the same bus does not answer the test.
    fn test_name() -> String {
        format!("dev.weegs.PeepersTest{}", std::process::id())
    }

    /// One test, because the name is the process's own.
    #[test]
    #[ignore = "needs a session bus"]
    fn a_command_reaches_the_listener_that_owns_the_name_and_only_one_can() {
        let name = test_name();
        let listener = Listener::claim(&name).unwrap();
        assert!(Listener::claim(&name).is_err());

        let commands = listener.commands();
        call(&name, Command::TogglePopout).unwrap();
        assert_eq!(commands.recv(), Some(Command::TogglePopout));

        drop(listener);
        assert!(call(&name, Command::TogglePopout).is_err());
    }
}
