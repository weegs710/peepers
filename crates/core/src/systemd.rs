use zbus::blocking::{Connection, Proxy};
use zbus::proxy::MethodFlags;
use zvariant::OwnedObjectPath;

const DESTINATION: &str = "org.freedesktop.systemd1";
const PATH: &str = "/org/freedesktop/systemd1";
const INTERFACE: &str = "org.freedesktop.systemd1.Manager";

/// Which systemd instance a unit belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bus {
    /// The system manager; changing a unit there asks polkit for permission.
    System,
    /// The calling user's own manager.
    User,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Active {
    Active,
    Activating,
    Deactivating,
    Reloading,
    Inactive,
    Failed,
    Other(String),
}

impl From<&str> for Active {
    fn from(state: &str) -> Self {
        match state {
            "active" => Self::Active,
            "activating" => Self::Activating,
            "deactivating" => Self::Deactivating,
            "reloading" => Self::Reloading,
            "inactive" => Self::Inactive,
            "failed" => Self::Failed,
            other => Self::Other(other.to_owned()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    /// The unit name, such as `sshd.service`.
    pub name: String,
    pub description: String,
    pub bus: Bus,
    pub load: String,
    pub active: Active,
    /// Finer detail than [`Active`], such as `running`, `exited` or `dead`.
    pub sub: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Start,
    Stop,
    Restart,
}

impl Action {
    fn method(self) -> &'static str {
        match self {
            Self::Start => "StartUnit",
            Self::Stop => "StopUnit",
            Self::Restart => "RestartUnit",
        }
    }
}

/// `name, description, load, active, sub, followed, path, job id, job type, job path`
type Unit = (
    String,
    String,
    String,
    String,
    String,
    String,
    OwnedObjectPath,
    u32,
    String,
    OwnedObjectPath,
);

/// Every loaded service of the system manager and of the user's own manager. The user manager may
/// be absent, such as outside a login session, and then contributes nothing.
///
/// # Errors
///
/// Fails when the system manager cannot be reached.
pub fn read() -> Result<Vec<Service>, String> {
    let mut services = list(Bus::System)?;
    services.extend(list(Bus::User).unwrap_or_default());
    services.sort_by(|a, b| a.name.cmp(&b.name).then(a.bus.cmp(&b.bus)));
    Ok(services)
}

fn list(bus: Bus) -> Result<Vec<Service>, String> {
    let units: Vec<Unit> = manager(bus)?
        .call("ListUnits", &())
        .map_err(|error| error.to_string())?;
    Ok(units
        .into_iter()
        .filter(|unit| unit.0.ends_with(".service"))
        .map(|(name, description, load, active, sub, ..)| Service {
            name,
            description,
            bus,
            load,
            active: Active::from(active.as_str()),
            sub,
        })
        .collect())
}

/// Starts, stops or restarts a service, letting polkit ask the user to authorize it when needed.
///
/// # Errors
///
/// Fails when the unit does not exist, or the user is not authorized or declines.
pub fn control(bus: Bus, name: &str, action: Action) -> Result<(), String> {
    manager(bus)?
        .call_with_flags::<_, _, OwnedObjectPath>(
            action.method(),
            MethodFlags::AllowInteractiveAuth.into(),
            &(name, "replace"),
        )
        .map(drop)
        .map_err(|error| error.to_string())
}

fn manager(bus: Bus) -> Result<Proxy<'static>, String> {
    let connection = match bus {
        Bus::System => Connection::system(),
        Bus::User => Connection::session(),
    }
    .map_err(|error| error.to_string())?;
    Proxy::new(&connection, DESTINATION, PATH, INTERFACE).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_map_from_their_names() {
        assert_eq!(Active::from("active"), Active::Active);
        assert_eq!(Active::from("failed"), Active::Failed);
        assert_eq!(
            Active::from("maintenance"),
            Active::Other("maintenance".to_owned())
        );
    }

    #[test]
    fn this_machine_lists_services_with_an_active_one() {
        let services = read().unwrap();
        assert!(
            services
                .iter()
                .all(|service| service.name.ends_with(".service"))
        );
        assert!(
            services
                .iter()
                .any(|service| service.active == Active::Active)
        );
    }

    #[test]
    fn a_unit_that_does_not_exist_cannot_be_started() {
        assert!(control(Bus::User, "peepers-no-such-unit.service", Action::Start).is_err());
    }
}
