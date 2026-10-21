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
    /// A close has been asked for and not yet reported.
    closing: bool,
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
            closing: false,
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
        self.closing = false;
        opened.discard()
    }

    /// Asks for the window to close. It counts as open until the close is reported, so what is
    /// asked of it in that gap must not take it for a window that will stay.
    fn close<T: Send + 'static>(&mut self, id: window::Id) -> Task<T> {
        self.closing = true;
        window::close(id)
    }

    /// Whether the window is open and not on its way out.
    fn staying(&self) -> bool {
        self.id.is_some() && !self.closing
    }

    /// Shows the window if it is hidden and hides it if it is shown.
    pub fn toggle<T: Send + 'static>(&mut self) -> Task<T> {
        match self.id {
            Some(id) if self.staying() => self.close(id),
            _ => self.open(),
        }
    }

    /// Opens the window unless it is open already. One on its way out is opened again.
    pub fn show<T: Send + 'static>(&mut self) -> Task<T> {
        if self.staying() {
            Task::none()
        } else {
            self.open()
        }
    }

    /// Closes the window if it is open.
    pub fn hide<T: Send + 'static>(&mut self) -> Task<T> {
        match self.id {
            Some(id) if self.staying() => self.close(id),
            _ => Task::none(),
        }
    }

    /// Brings the window to the user, or hides it when they are already looking at it. One that
    /// is open without the focus is opened again, because `gain_focus` did nothing under umbriel
    /// and a new window is what a compositor focuses.
    pub fn activate<T: Send + 'static>(&mut self) -> Task<T> {
        match self.id {
            Some(id) if self.staying() && self.focused => self.close(id),
            Some(id) if self.staying() => window::close(id).chain(self.open()),
            _ => self.open(),
        }
    }

    /// Brings the window to the user without ever hiding it: opened when closed, left alone when
    /// it has the focus, and opened again when it is open without it, as [`Window::activate`] does.
    pub fn reveal<T: Send + 'static>(&mut self) -> Task<T> {
        match self.id {
            Some(_) if self.staying() && self.focused => Task::none(),
            Some(id) if self.staying() => window::close(id).chain(self.open()),
            _ => self.open(),
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
                    self.close(id)
                } else {
                    iced::exit()
                };
            }
            Event::Closed(id) if self.owns(id) => {
                self.id = None;
                self.focused = false;
                self.closing = false;
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
    fn revealing_opens_a_closed_window_and_never_closes_one() {
        let mut window = Window::new(Kind::Main);
        assert!(window.reveal::<()>().units() > 0);
        assert!(window.is_open());
        let id = window.id.unwrap();
        drop(window.update::<()>(Event::Focused(id), true));
        assert_eq!(window.reveal::<()>().units(), 0);
        assert_eq!(window.id, Some(id));
    }

    #[test]
    fn revealing_an_unfocused_window_replaces_it() {
        let (mut window, id) = open();
        drop(window.reveal::<()>());
        assert!(window.is_open() && window.id != Some(id));
    }

    #[test]
    fn hiding_closes_an_open_window_once_and_leaves_a_closed_one_alone() {
        let mut window = Window::new(Kind::Main);
        assert_eq!(window.hide::<()>().units(), 0);
        drop(window.open::<()>());
        let id = window.id.unwrap();
        assert!(window.hide::<()>().units() > 0);
        assert_eq!(window.hide::<()>().units(), 0);
        drop(window.update::<()>(Event::Closed(id), true));
        assert_eq!(window.hide::<()>().units(), 0);
    }

    #[test]
    fn showing_opens_a_window_that_is_not_staying_and_no_other() {
        let (mut window, id) = open();
        assert_eq!(window.show::<()>().units(), 0);
        assert_eq!(window.id, Some(id));
        drop(window.hide::<()>());
        assert!(window.show::<()>().units() > 0);
        assert_ne!(window.id, Some(id));
        drop(window.update::<()>(Event::Closed(id), true));
        assert!(window.is_open());
    }

    #[test]
    fn toggling_a_window_on_its_way_out_opens_it_again() {
        let (mut window, id) = open();
        drop(window.toggle::<()>());
        drop(window.toggle::<()>());
        assert_ne!(window.id, Some(id));
        drop(window.update::<()>(Event::Closed(id), true));
        assert!(window.is_open());
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
