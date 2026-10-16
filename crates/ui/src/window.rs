use iced::window::settings::PlatformSpecific;
use iced::{Size, Subscription, Task, event, window};

const SIZE: Size = Size::new(1280.0, 800.0);

/// The id lets a compositor rule find the window, which is how it is made to float.
const APPLICATION_ID: &str = "peepers";

fn settings() -> window::Settings {
    window::Settings {
        size: SIZE,
        exit_on_close_request: false,
        platform_specific: PlatformSpecific {
            application_id: APPLICATION_ID.to_owned(),
            ..PlatformSpecific::default()
        },
        ..window::Settings::default()
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

/// The one window, which can be closed while the application runs on in the tray.
#[derive(Debug, Default)]
pub struct Window {
    id: Option<window::Id>,
    focused: bool,
}

impl Window {
    pub fn is_open(&self) -> bool {
        self.id.is_some()
    }

    pub fn open<T: Send + 'static>(&mut self) -> Task<T> {
        let (id, opened) = window::open(settings());
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
            Event::Focused(id) if self.id == Some(id) => self.focused = true,
            Event::Unfocused(id) if self.id == Some(id) => self.focused = false,
            Event::CloseRequested(id) => {
                return if to_tray {
                    window::close(id)
                } else {
                    iced::exit()
                };
            }
            Event::Closed(id) if self.id == Some(id) => {
                self.id = None;
                self.focused = false;
            }
            Event::Focused(_) | Event::Unfocused(_) | Event::Closed(_) => {}
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> (Window, window::Id) {
        let mut window = Window::default();
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
