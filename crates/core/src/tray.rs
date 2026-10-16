#![expect(
    clippy::used_underscore_binding,
    reason = "the interface macro reads the parameters it passes to each method back by name"
)]

mod icon;
mod menu;

use std::hash::{Hash, Hasher};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use zbus::blocking::{Connection, connection};
use zbus::interface;
use zvariant::ObjectPath;

use self::icon::Pixmap;
use self::menu::Menu;

pub use self::icon::Gauges;
pub use self::menu::Event;

const PATH: &str = "/StatusNotifierItem";
const ITEM: &str = "org.kde.StatusNotifierItem";
const WATCHER: &str = "org.kde.StatusNotifierWatcher";

/// The least time between two announcements: the host rebuilds the entry for each one, and an
/// entry that is rebuilt under the pointer cannot be clicked.
const PERIOD: Duration = Duration::from_secs(1);

/// The `StatusNotifierItem` a tray host draws.
struct Item {
    gauges: Gauges,
    events: Sender<Event>,
}

#[expect(
    clippy::unused_self,
    reason = "the interface macro exposes only methods, and most properties are fixed"
)]
#[interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    #[zbus(property)]
    fn category(&self) -> &'static str {
        "SystemServices"
    }

    #[zbus(property)]
    fn id(&self) -> &'static str {
        "peepers"
    }

    #[zbus(property)]
    fn title(&self) -> &'static str {
        "peepers"
    }

    #[zbus(property)]
    fn status(&self) -> &'static str {
        "Active"
    }

    /// False, so that a left click activates the item and only a right click opens the menu.
    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn menu(&self) -> ObjectPath<'static> {
        ObjectPath::from_static_str_unchecked(menu::PATH)
    }

    /// Empty so that hosts, which prefer a name over pixels, draw the gauges.
    #[zbus(property)]
    fn icon_name(&self) -> &'static str {
        ""
    }

    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<Pixmap> {
        vec![self.gauges.pixmap()]
    }

    #[zbus(property)]
    fn tool_tip(&self) -> (String, Vec<Pixmap>, String, String) {
        (
            String::new(),
            Vec::new(),
            "peepers".to_owned(),
            self.gauges.to_string(),
        )
    }

    /// A left click.
    fn activate(&self, _x: i32, _y: i32) {
        let _ = self.events.send(Event::Activate);
    }
}

/// Everything the tray shows, which the application changes a part of at a time.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct State {
    gauges: Gauges,
    window_open: bool,
    popout_open: bool,
    paused: bool,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The clicks that reach the tray, which a subscription can wait on and tell apart from another
/// tray's by identity.
#[derive(Debug, Clone)]
pub struct Events(Arc<Mutex<Receiver<Event>>>);

impl Events {
    /// Waits for the next click, or returns `None` once the tray is gone.
    #[must_use]
    pub fn recv(&self) -> Option<Event> {
        locked(&self.0).recv().ok()
    }
}

impl Hash for Events {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

fn service_name() -> String {
    format!("org.kde.StatusNotifierItem-{}-1", std::process::id())
}

/// A tray icon that gauges the machine and offers a menu. It leaves the tray when dropped.
#[derive(Debug)]
pub struct Tray {
    state: Arc<Mutex<State>>,
    wake: SyncSender<()>,
    events: Events,
}

impl Tray {
    /// # Errors
    ///
    /// Fails when there is no session bus or no tray host to register with.
    pub fn start() -> Result<Self, String> {
        let (clicks, received) = mpsc::channel();
        let name = service_name();
        let connection = connection::Builder::session()
            .and_then(|builder| builder.name(name.as_str()))
            .and_then(|builder| {
                builder.serve_at(
                    PATH,
                    Item {
                        gauges: Gauges::default(),
                        events: clicks.clone(),
                    },
                )
            })
            .and_then(|builder| {
                builder.serve_at(
                    menu::PATH,
                    Menu {
                        window_open: false,
                        popout_open: false,
                        paused: false,
                        revision: 0,
                        events: clicks,
                    },
                )
            })
            .and_then(connection::Builder::build)
            .map_err(|error| error.to_string())?;
        connection
            .call_method(
                Some(WATCHER),
                "/StatusNotifierWatcher",
                Some(WATCHER),
                "RegisterStatusNotifierItem",
                &name.as_str(),
            )
            .map_err(|error| format!("No tray to show the icon in: {error}"))?;

        let state = Arc::new(Mutex::new(State::default()));
        let (wake, woken) = mpsc::sync_channel(1);
        let published = Arc::clone(&state);
        thread::spawn(move || publish(&connection, &published, &woken));
        Ok(Self {
            state,
            wake,
            events: Events(Arc::new(Mutex::new(received))),
        })
    }

    #[must_use]
    pub fn events(&self) -> Events {
        self.events.clone()
    }

    pub fn show(&self, gauges: Gauges) {
        self.change(|state| state.gauges = gauges);
    }

    /// Whether the window is open, which the menu's first entry offers to reverse.
    pub fn set_window_open(&self, open: bool) {
        self.change(|state| state.window_open = open);
    }

    /// Whether the popout is open, which the menu's second entry offers to reverse.
    pub fn set_popout_open(&self, open: bool) {
        self.change(|state| state.popout_open = open);
    }

    pub fn set_paused(&self, paused: bool) {
        self.change(|state| state.paused = paused);
    }

    /// A wake that finds one already waiting is dropped, because whoever handles that one reads
    /// the latest state.
    fn change(&self, apply: impl FnOnce(&mut State)) {
        apply(&mut locked(&self.state));
        let _ = self.wake.try_send(());
    }
}

/// Brings the item and menu up to date with the state and tells the host what changed, until the
/// tray is dropped or the bus goes away. Only what actually differs is announced, and not more
/// often than [`PERIOD`] however fast the state changes.
fn publish(connection: &Connection, state: &Mutex<State>, woken: &Receiver<()>) {
    let server = connection.object_server();
    let (Ok(item), Ok(menu)) = (
        server.interface::<_, Item>(PATH),
        server.interface::<_, Menu>(menu::PATH),
    ) else {
        return;
    };
    let mut shown = State::default();
    for () in woken {
        let now = *locked(state);
        if now == shown {
            continue;
        }
        let icon = now.gauges.pixmap() != shown.gauges.pixmap();
        let tooltip = now.gauges.to_string() != shown.gauges.to_string();
        let entries = (now.window_open, now.popout_open, now.paused)
            != (shown.window_open, shown.popout_open, shown.paused);
        item.get_mut().gauges = now.gauges;
        let revision = {
            let mut menu = menu.get_mut();
            menu.window_open = now.window_open;
            menu.popout_open = now.popout_open;
            menu.paused = now.paused;
            if entries {
                menu.revision += 1;
            }
            menu.revision
        };
        shown = now;

        let sent = [
            icon.then(|| connection.emit_signal(None::<&str>, PATH, ITEM, "NewIcon", &())),
            tooltip.then(|| connection.emit_signal(None::<&str>, PATH, ITEM, "NewToolTip", &())),
            entries.then(|| {
                connection.emit_signal(
                    None::<&str>,
                    menu::PATH,
                    menu::INTERFACE,
                    "LayoutUpdated",
                    &(revision, menu::ROOT),
                )
            }),
        ];
        if sent.iter().flatten().any(Result::is_err) {
            return;
        }
        thread::sleep(PERIOD);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use zbus::blocking::Proxy;
    use zvariant::{OwnedValue, Value};

    use super::*;

    /// Reads the item the way a host does, with no cache between the test and the bus.
    fn item(bus: &Connection) -> Proxy<'static> {
        proxy(bus, PATH, ITEM, zbus::proxy::CacheProperties::No)
    }

    fn proxy(
        bus: &Connection,
        path: &'static str,
        interface: &'static str,
        cache: zbus::proxy::CacheProperties,
    ) -> Proxy<'static> {
        zbus::blocking::proxy::Builder::new(bus)
            .destination(service_name())
            .unwrap()
            .path(path)
            .unwrap()
            .interface(interface)
            .unwrap()
            .cache_properties(cache)
            .build()
            .unwrap()
    }

    /// Longer than [`PERIOD`], so an announcement held back by the last one has gone out.
    fn settle() {
        thread::sleep(PERIOD + Duration::from_millis(300));
    }

    type Reply = (u32, (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>));

    /// One test, because two trays in a process would claim the same bus name.
    #[test]
    #[ignore = "needs a tray host on the session bus"]
    fn the_host_sees_the_item_and_its_clicks_reach_the_application() {
        let tray = Tray::start().unwrap();
        let bus = Connection::session().unwrap();
        let watcher = Proxy::new(&bus, WATCHER, "/StatusNotifierWatcher", WATCHER).unwrap();
        let items: Vec<String> = watcher
            .get_property("RegisteredStatusNotifierItems")
            .unwrap();
        assert!(
            items.iter().any(|item| item.contains(&service_name())),
            "{items:?}"
        );

        let item = item(&bus);
        let icon = || item.get_property::<Vec<Pixmap>>("IconPixmap").unwrap();
        assert_eq!(icon(), [Gauges::default().pixmap()]);
        let busy = Gauges {
            cpu: 100.0,
            memory: 90.0,
        };
        tray.show(busy);
        settle();
        assert_eq!(icon(), [busy.pixmap()]);

        let events = tray.events();
        item.call_method("Activate", &(0_i32, 0_i32)).unwrap();
        assert_eq!(events.recv(), Some(Event::Activate));

        let menu = proxy(
            &bus,
            menu::PATH,
            menu::INTERFACE,
            zbus::proxy::CacheProperties::No,
        );
        let layout = |paused: bool| {
            tray.set_paused(paused);
            settle();
            let reply: Reply = menu
                .call("GetLayout", &(menu::ROOT, -1_i32, Vec::<&str>::new()))
                .unwrap();
            reply
        };
        let (before, (_, _, children)) = layout(false);
        assert_eq!(children.len(), 5);
        let (after, _) = layout(true);
        assert_eq!(after, before + 1);

        menu.call_method("Event", &(3_i32, "clicked", Value::from(0_i32), 0_u32))
            .unwrap();
        assert_eq!(events.recv(), Some(Event::Pause));

        let (arrivals, announced) = mpsc::channel();
        let listener = bus.clone();
        thread::spawn(move || {
            let rule = zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface(ITEM)
                .unwrap()
                .member("NewIcon")
                .unwrap()
                .build();
            let messages = zbus::blocking::MessageIterator::for_match_rule(rule, &listener, None);
            for _ in messages.unwrap().flatten() {
                let _ = arrivals.send(());
            }
        });
        settle();
        while announced.try_recv().is_ok() {}
        for step in 0..10_u8 {
            tray.show(Gauges {
                cpu: f32::from(step) * 10.0,
                memory: 0.0,
            });
            thread::sleep(Duration::from_millis(100));
        }
        settle();
        let count = announced.try_iter().count();
        assert!((1..=3).contains(&count), "{count} announcements");
    }
}
