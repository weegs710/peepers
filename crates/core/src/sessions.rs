use zbus::blocking::{Connection, Proxy};
use zbus::proxy::MethodFlags;
use zvariant::OwnedObjectPath;

const DESTINATION: &str = "org.freedesktop.login1";
const PATH: &str = "/org/freedesktop/login1";
const INTERFACE: &str = "org.freedesktop.login1.Manager";

/// A login session known to `logind`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub uid: u32,
    pub user: String,
    pub seat: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Lock,
    SignOff,
}

/// `session id, user id, user name, seat id, object path`
type Entry = (String, u32, String, String, OwnedObjectPath);

/// # Errors
///
/// Fails when `logind` cannot be reached.
pub fn read() -> Result<Vec<Session>, String> {
    let entries: Vec<Entry> = manager()?
        .call("ListSessions", &())
        .map_err(|error| error.to_string())?;
    Ok(entries
        .into_iter()
        .map(|(id, uid, user, seat, _)| Session {
            id,
            uid,
            user,
            seat: (!seat.is_empty()).then_some(seat),
        })
        .collect())
}

/// Locks a session, or ends it, letting polkit ask the user to authorize it when needed.
///
/// # Errors
///
/// Fails when the session is gone, or the user is not authorized or declines.
pub fn control(id: &str, action: Action) -> Result<(), String> {
    let method = match action {
        Action::Lock => "LockSession",
        Action::SignOff => "TerminateSession",
    };
    manager()?
        .call_with_flags::<_, _, ()>(method, MethodFlags::AllowInteractiveAuth.into(), &(id,))
        .map(drop)
        .map_err(|error| error.to_string())
}

fn manager() -> Result<Proxy<'static>, String> {
    let connection = Connection::system().map_err(|error| error.to_string())?;
    Proxy::new(&connection, DESTINATION, PATH, INTERFACE).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_machine_has_a_session_for_this_user() {
        let sessions = read().unwrap();
        assert_ne!(sessions.len(), 0);
        assert!(
            sessions
                .iter()
                .all(|session| !session.id.is_empty() && !session.user.is_empty())
        );
    }

    #[test]
    fn a_session_that_does_not_exist_cannot_be_locked() {
        assert!(control("peepers-no-such-session", Action::Lock).is_err());
    }
}
