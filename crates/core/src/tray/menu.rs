#![expect(
    clippy::used_underscore_binding,
    reason = "the interface macro reads the parameters it passes to each method back by name"
)]

use std::collections::HashMap;
use std::sync::mpsc::Sender;

use zbus::interface;
use zvariant::{Structure, Value};

pub(super) const PATH: &str = "/MenuBar";
pub(super) const INTERFACE: &str = "com.canonical.dbusmenu";
pub(super) const ROOT: i32 = 0;

const WINDOW: i32 = 1;
const POPOUT: i32 = 2;
const PAUSE: i32 = 3;
const SEPARATOR: i32 = 4;
const QUIT: i32 = 5;

/// What a click in the tray asks of the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A left click on the icon: bring the window to the user, or put it away when they are
    /// already looking at it.
    Activate,
    /// The menu's first entry: show the window if it is hidden, hide it if it is shown.
    Window,
    /// Open the popout window of live gauges, or close it when it is open.
    Popout,
    Pause,
    Quit,
}

type Properties = HashMap<String, Value<'static>>;
type Layout = (i32, Properties, Vec<Value<'static>>);

/// The context menu a tray host draws, over the `dbusmenu` protocol.
pub(super) struct Menu {
    pub(super) window_open: bool,
    pub(super) popout_open: bool,
    pub(super) paused: bool,
    /// Tells the host the layout changed, so it asks for it again.
    pub(super) revision: u32,
    pub(super) events: Sender<Event>,
}

fn labelled(label: &'static str) -> Properties {
    Properties::from([("label".to_owned(), Value::from(label))])
}

impl Menu {
    fn entries(&self) -> [(i32, Properties); 5] {
        [
            (
                WINDOW,
                labelled(if self.window_open {
                    "Hide to tray"
                } else {
                    "Show peepers"
                }),
            ),
            (
                POPOUT,
                labelled(if self.popout_open {
                    "Close popout"
                } else {
                    "Open popout"
                }),
            ),
            (
                PAUSE,
                labelled(if self.paused {
                    "Resume updates"
                } else {
                    "Pause updates"
                }),
            ),
            (
                SEPARATOR,
                Properties::from([("type".to_owned(), Value::from("separator"))]),
            ),
            (QUIT, labelled("Quit peepers")),
        ]
    }
}

#[expect(
    clippy::unused_self,
    clippy::needless_pass_by_value,
    reason = "the interface macro fixes each method's signature"
)]
#[interface(name = "com.canonical.dbusmenu")]
impl Menu {
    /// The menu is one level deep, so every item is its own whole subtree.
    fn get_layout(
        &self,
        parent_id: i32,
        _recursion_depth: i32,
        _property_names: Vec<String>,
    ) -> (u32, Layout) {
        let layout = if parent_id == ROOT {
            let children = self
                .entries()
                .into_iter()
                .map(|(id, properties)| {
                    Value::Structure(Structure::from((
                        id,
                        properties,
                        Vec::<Value<'static>>::new(),
                    )))
                })
                .collect();
            (
                ROOT,
                Properties::from([("children-display".to_owned(), Value::from("submenu"))]),
                children,
            )
        } else {
            let properties = self
                .entries()
                .into_iter()
                .find_map(|(id, properties)| (id == parent_id).then_some(properties))
                .unwrap_or_default();
            (parent_id, properties, Vec::new())
        };
        (self.revision, layout)
    }

    /// An empty list asks for every item.
    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        _property_names: Vec<String>,
    ) -> Vec<(i32, Properties)> {
        self.entries()
            .into_iter()
            .filter(|(id, _)| ids.is_empty() || ids.contains(id))
            .collect()
    }

    fn about_to_show(&self, _id: i32) -> bool {
        false
    }

    fn event(&self, id: i32, event_id: &str, _data: Value<'_>, _timestamp: u32) {
        let event = match (event_id, id) {
            ("clicked", WINDOW) => Event::Window,
            ("clicked", POPOUT) => Event::Popout,
            ("clicked", PAUSE) => Event::Pause,
            ("clicked", QUIT) => Event::Quit,
            _ => return,
        };
        let _ = self.events.send(event);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{self, Receiver};

    use super::*;

    fn menu(window_open: bool, popout_open: bool, paused: bool) -> (Menu, Receiver<Event>) {
        let (events, clicks) = mpsc::channel();
        (
            Menu {
                window_open,
                popout_open,
                paused,
                revision: 7,
                events,
            },
            clicks,
        )
    }

    fn label(properties: &Properties) -> Option<String> {
        properties
            .get("label")
            .and_then(|value| String::try_from(value.clone()).ok())
    }

    #[test]
    fn the_root_lists_five_items_under_the_current_revision() {
        let (menu, _clicks) = menu(true, false, false);
        let (revision, (id, _, children)) = menu.get_layout(ROOT, -1, Vec::new());
        assert_eq!((revision, id, children.len()), (7, ROOT, 5));
    }

    #[test]
    fn labels_follow_the_state() {
        let (open, _clicks) = menu(true, true, true);
        let labels: Vec<_> = open
            .get_group_properties(Vec::new(), Vec::new())
            .iter()
            .map(|(_, properties)| label(properties))
            .collect();
        assert_eq!(
            labels,
            [
                Some("Hide to tray".to_owned()),
                Some("Close popout".to_owned()),
                Some("Resume updates".to_owned()),
                None,
                Some("Quit peepers".to_owned()),
            ]
        );
        let (hidden, _clicks) = menu(false, false, false);
        let toggles = hidden.get_group_properties(vec![WINDOW, POPOUT], Vec::new());
        assert_eq!(label(&toggles[0].1).as_deref(), Some("Show peepers"));
        assert_eq!(label(&toggles[1].1).as_deref(), Some("Open popout"));
    }

    #[test]
    fn an_item_is_a_subtree_without_children() {
        let (menu, _clicks) = menu(true, false, false);
        let (_, (id, properties, children)) = menu.get_layout(QUIT, 0, Vec::new());
        assert_eq!(id, QUIT);
        assert_eq!(label(&properties).as_deref(), Some("Quit peepers"));
        assert!(children.is_empty());
    }

    #[test]
    fn clicks_become_events_and_nothing_else_does() {
        let (menu, clicks) = menu(true, false, false);
        for (id, kind) in [
            (WINDOW, "clicked"),
            (POPOUT, "clicked"),
            (PAUSE, "clicked"),
            (QUIT, "clicked"),
        ] {
            menu.event(id, kind, Value::from(0_i32), 0);
        }
        menu.event(PAUSE, "hovered", Value::from(0_i32), 0);
        menu.event(SEPARATOR, "clicked", Value::from(0_i32), 0);
        assert_eq!(
            clicks.try_iter().collect::<Vec<_>>(),
            [Event::Window, Event::Popout, Event::Pause, Event::Quit]
        );
    }
}
