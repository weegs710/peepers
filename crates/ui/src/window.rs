use iced::window::settings::PlatformSpecific;
use iced::{Size, Subscription, Task, event, window};

/// Which of the application's windows a [`Window`] keeps track of.
#[derive(Debug, Clone, Copy)]
enum Kind {
    Main,
    /// A narrow window of live gauges that stays beside the work.
    Popout,
}

impl Kind {
    /// The application id lets a compositor rule find the window, which is how it is made to
    /// float, so each kind has its own because a gauge window wants a size of its own.
    fn settings(self) -> window::Settings {
        let (size, application_id) = match self {
            Self::Main => (Size::new(1280.0, 800.0), "peepers"),
            Self::Popout => (Size::new(300.0, 560.0), "peepers-popout"),
        };
        window::Settings {
            size,
            exit_on_close_request: false,
            platform_specific: PlatformSpecific {
                application_id: application_id.to_owned(),
                ..PlatformSpecific::default()
            },
            ..window::Settings::default()
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Event {
    Focused(window::Id),
    Unfocused(window::Id),
    CloseRequested(window::Id),
    Closed(window::Id),
}

pub fn events() -> Subscription<Event> {
    event::listen_with(|event, _status, id| match event {
        iced::Event::Window(window::Event::Focused) => Some(Event::Focused(id)),
        iced::Event::Window(window::Event::Unfocused) => Some(Event::Unfocused(id)),
        iced::Event::Window(window::Event::CloseRequested) => Some(Event::CloseRequested(id)),
        iced::Event::Window(window::Event::Closed) => Some(Event::Closed(id)),
        _ => None,
    })
}

/// A window that can be closed while the application runs on in the tray.
#[derive(Debug)]
pub struct Window {
    kind: Kind,
    id: Option<window::Id>,
    focused: bool,
}

/// The application's windows, which share the clock and the history but not the room.
#[derive(Debug)]
pub struct Windows {
    pub main: Window,
    pub popout: Window,
}

impl Default for Windows {
    fn default() -> Self {
        Self {
            main: Window::new(Kind::Main),
            popout: Window::new(Kind::Popout),
        }
    }
}

impl Window {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            id: None,
            focused: false,
        }
    }

    pub fn is_open(&self) -> bool {
        self.id.is_some()
    }

    /// Whether `id` is the window that is open now.
    pub fn owns(&self, id: window::Id) -> bool {
        self.id == Some(id)
    }

    #[cfg(test)]
    pub fn id(&self) -> Option<window::Id> {
        self.id
    }

    pub fn open<T: Send + 'static>(&mut self) -> Task<T> {
        let (id, opened) = window::open(self.kind.settings());
        self.id = Some(id);
        opened.discard()
    }

    /// Shows the window if it is hidden and hides it if it is shown.
    pub fn toggle<T: Send + 'static>(&mut self) -> Task<T> {
        match self.id {
            None => self.open(),
            Some(id) => window::close(id),
        }
    }

    /// Brings the window to the user, or hides it when they are already looking at it. One that
    /// is open without the focus is opened again, because `gain_focus` did nothing under umbriel
    /// and a new window is what a compositor focuses.
    pub fn activate<T: Send + 'static>(&mut self) -> Task<T> {
        match self.id {
            None => self.open(),
            Some(id) if self.focused => window::close(id),
            Some(id) => window::close(id).chain(self.open()),
        }
    }

    /// With `to_tray` a close request hides the window, and without it the application ends,
    /// because nothing would be left to bring the window back.
    pub fn update<T: Send + 'static>(&mut self, event: Event, to_tray: bool) -> Task<T> {
        match event {
            Event::Focused(id) if self.owns(id) => self.focused = true,
            Event::Unfocused(id) if self.owns(id) => self.focused = false,
            Event::CloseRequested(id) if self.owns(id) => {
                return if to_tray {
                    window::close(id)
                } else {
                    iced::exit()
                };
            }
            Event::Closed(id) if self.owns(id) => {
                self.id = None;
                self.focused = false;
            }
            Event::Focused(_)
            | Event::Unfocused(_)
            | Event::CloseRequested(_)
            | Event::Closed(_) => {}
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> (Window, window::Id) {
        let mut window = Window::new(Kind::Main);
        drop(window.open::<()>());
        let id = window.id.unwrap();
        (window, id)
    }

    #[test]
    fn a_closed_window_is_noted() {
        let (mut window, id) = open();
        assert!(window.is_open());
        drop(window.update::<()>(Event::Closed(id), true));
        assert!(!window.is_open());
    }

    #[test]
    fn activating_an_unfocused_window_replaces_it() {
        let (mut window, id) = open();
        drop(window.activate::<()>());
        assert!(window.is_open() && window.id != Some(id));
        drop(window.update::<()>(Event::Closed(id), true));
        assert!(window.is_open());
    }

    #[test]
    fn activating_a_focused_window_leaves_it_to_close() {
        let (mut window, id) = open();
        drop(window.update::<()>(Event::Focused(id), true));
        drop(window.activate::<()>());
        assert_eq!(window.id, Some(id));
    }

    #[test]
    fn toggling_closes_a_window_whether_or_not_it_has_the_focus() {
        let (mut window, id) = open();
        drop(window.toggle::<()>());
        assert_eq!(window.id, Some(id));
        drop(window.update::<()>(Event::Closed(id), true));
        drop(window.toggle::<()>());
        assert!(window.is_open() && window.id != Some(id));
    }

    #[test]
    fn another_windows_events_are_ignored() {
        let (mut window, _) = open();
        let stranger = window::Id::unique();
        drop(window.update::<()>(Event::Focused(stranger), true));
        assert!(!window.focused);
        drop(window.update::<()>(Event::Closed(stranger), true));
        assert!(window.is_open());
    }

    #[test]
    fn focus_follows_the_events() {
        let (mut window, id) = open();
        drop(window.update::<()>(Event::Focused(id), true));
        assert!(window.focused);
        drop(window.update::<()>(Event::Unfocused(id), true));
        assert!(!window.focused);
    }
}
