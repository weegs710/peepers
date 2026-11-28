use crate::theme::Element;
use crate::theme::styles;
use std::time::{Duration, Instant};

use iced::Fill;
use iced::widget::{button, row, text};

/// How long a success stays on screen.
const LINGER: Duration = Duration::from_secs(5);

/// How often a page with a success on screen is drawn again, so the success leaves on time.
pub const REFRESH: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Something the user asked for went through, and it goes away by itself.
    Success,
    /// Something to know about that stays until it is dismissed.
    Note,
    /// Something the user asked for failed, and it stays until it is dismissed.
    Failure,
}

/// A line a page shows about what just happened.
#[derive(Debug, Clone)]
pub struct Notice {
    text: String,
    kind: Kind,
    at: Instant,
}

impl Notice {
    pub fn success(text: impl Into<String>) -> Self {
        Self::new(text.into(), Kind::Success)
    }

    pub fn note(text: impl Into<String>) -> Self {
        Self::new(text.into(), Kind::Note)
    }

    pub fn failure(text: impl Into<String>) -> Self {
        Self::new(text.into(), Kind::Failure)
    }

    fn new(text: String, kind: Kind) -> Self {
        Self {
            text,
            kind,
            at: Instant::now(),
        }
    }

    /// Whether a page should still show it.
    pub fn showing(&self) -> bool {
        self.kind != Kind::Success || self.fading()
    }

    /// Whether it is a success that has not left yet, so the page has to be drawn again until it
    /// has.
    pub fn fading(&self) -> bool {
        self.kind == Kind::Success && self.at.elapsed() < LINGER
    }

    /// The line, red for a failure and green otherwise, with a button to dismiss it when it would
    /// otherwise stay.
    pub fn view<'a, Message: Clone + 'a>(&'a self, dismiss: Message) -> Element<'a, Message> {
        let line = text(&self.text)
            .width(Fill)
            .style(if self.kind == Kind::Failure {
                styles::text_danger
            } else {
                styles::text_success
            });
        if self.kind == Kind::Success {
            line.into()
        } else {
            row![
                line,
                button("Dismiss")
                    .on_press(dismiss)
                    .style(styles::button_text)
            ]
            .spacing(8)
            .align_y(iced::Center)
            .into()
        }
    }

    /// The same notice shown a while ago, to test what a page does once it has run its time.
    #[cfg(test)]
    pub fn aged(mut self) -> Self {
        self.at = self
            .at
            .checked_sub(LINGER + Duration::from_secs(1))
            .unwrap_or(self.at);
        self
    }
}

/// The notice if there is one and it is still to be shown.
pub fn shown(notice: Option<&Notice>) -> Option<&Notice> {
    notice.filter(|notice| notice.showing())
}

/// Whether a page's notice is a success that is about to leave.
pub fn fading(notice: Option<&Notice>) -> bool {
    notice.is_some_and(Notice::fading)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_success_shows_at_first_and_leaves_by_itself() {
        let notice = Notice::success("Requested");
        assert!(notice.showing() && notice.fading());
        let later = notice.aged();
        assert!(!later.showing() && !later.fading());
    }

    #[test]
    fn a_failure_or_a_note_stays_however_long_it_has_been() {
        for notice in [Notice::failure("It broke"), Notice::note("Heads up")] {
            let notice = notice.aged();
            assert!(notice.showing());
            assert!(!notice.fading());
        }
    }

    #[test]
    fn a_page_shows_only_what_is_still_to_be_shown() {
        let fresh = Notice::success("Saved");
        assert!(shown(Some(&fresh)).is_some());
        let old = Notice::success("Saved").aged();
        assert!(shown(Some(&old)).is_none());
        assert!(shown(None).is_none());
        assert!(fading(Some(&fresh)) && !fading(Some(&old)) && !fading(None));
    }
}
