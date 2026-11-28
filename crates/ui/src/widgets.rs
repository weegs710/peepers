use crate::theme::styles;
use crate::theme::{Element, Theme};
use std::cmp::Ordering;

use iced::widget::canvas::{self, Frame, Geometry, Path};
use iced::widget::grid::Sizing;
use iced::widget::text::Wrapping;
use iced::widget::{
    Scrollable, button, column, container, grid, responsive, row, scrollable, space, text,
};
use iced::{Fill, Length, Point, Rectangle, Renderer, mouse};

use peepers_core::telemetry::Vitals;

use crate::graph::Graph;

/// Space kept between a scrollbar and the content, so the bar never covers the last column.
const SCROLLBAR_GAP: f32 = 6.0;

/// A scrollable whose bar has a strip of its own instead of floating over the content.
pub fn scrolling<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> Scrollable<'a, Message, Theme> {
    scrollable(content).spacing(SCROLLBAR_GAP)
}

/// The narrowest a captioned value is drawn: wide enough for the longest word a value is made of.
const STAT_WIDTH: f32 = 200.0;
const STAT_SPACING: f32 = 16.0;

/// The most of a window's height a detail page gives its facts before they scroll, so a long list
/// of them cannot squeeze the graph away.
const FACTS_SHARE: f32 = 0.5;

/// How many cells of at least `least` fit across `width` with `spacing` between them.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the count is floored and never below one"
)]
fn columns(width: f32, least: f32, spacing: f32) -> usize {
    ((width + spacing) / (least + spacing)).floor().max(1.0) as usize
}

/// Cells laid across the width they are given, as many to a row as hold `least` each and stretched
/// to fill it, so a wide window leaves no strip over and a narrow one wraps. The cells are built
/// again whenever the width is measured.
pub fn flow<'a, Message: 'a>(
    least: f32,
    spacing: f32,
    cells: impl Fn() -> Vec<Element<'a, Message>> + 'a,
) -> Element<'a, Message> {
    responsive(move |size| {
        grid(cells())
            .columns(columns(size.width, least, spacing))
            .spacing(spacing)
            .height(Sizing::EvenlyDistribute(Length::Shrink))
            .into()
    })
    .height(Length::Shrink)
    .into()
}

/// A caption above a value.
pub fn stat<'a, Message: 'a>(
    label: impl Into<String>,
    value: impl Into<String>,
) -> Element<'a, Message> {
    column![
        text(label.into()).size(12).style(styles::label),
        text(value.into())
            .size(18)
            .style(styles::value)
            .wrapping(Wrapping::WordOrGlyph)
    ]
    .spacing(2)
    .into()
}

/// Captioned values flowing left to right and wrapping at the edge.
pub fn stats<'a, Message: 'a, L, V>(items: impl IntoIterator<Item = (L, V)>) -> Element<'a, Message>
where
    L: Into<String>,
    V: Into<String>,
{
    let items: Vec<(String, String)> = items
        .into_iter()
        .map(|(label, value)| (label.into(), value.into()))
        .collect();
    flow(STAT_WIDTH, STAT_SPACING, move || {
        items
            .iter()
            .map(|(label, value)| stat(label.as_str(), value.as_str()))
            .collect()
    })
}

/// One light per disk, lit while the disk moved data in the latest sample.
pub fn disk_lights<'a, Message: 'a>(vitals: &Vitals) -> Element<'a, Message> {
    column(vitals.disks.iter().map(|disk| {
        let busy = disk.read_per_second + disk.write_per_second > 0;
        row![
            text("●").style(if busy {
                styles::text_success
            } else {
                styles::text_secondary
            }),
            text(disk.name.clone()).size(12),
        ]
        .spacing(6)
        .into()
    }))
    .spacing(2)
    .padding(8)
    .into()
}

/// Title and subtitle over a graph that takes the free height, with a block of facts below. The
/// facts get at most half of the `available` height and scroll beyond that.
pub fn detail<'a, Message: 'a>(
    title: impl Into<String>,
    subtitle: Option<String>,
    graph: Graph,
    available: f32,
    facts: Element<'a, Message>,
) -> Element<'a, Message> {
    column![
        row![
            text(title.into()).size(26).style(styles::heading),
            space::horizontal(),
            text(subtitle.unwrap_or_default()).style(styles::text_secondary)
        ],
        container(graph.view()).height(Fill).width(Fill),
        container(scrolling(facts).width(Fill)).max_height(available * FACTS_SHARE),
    ]
    .spacing(14)
    .padding(16)
    .height(Fill)
    .into()
}

/// A column a table can be ordered by.
pub trait Sortable: Copy + PartialEq {
    /// Whether a first press on the column puts the largest first. Numbers read best that way and
    /// names the other way round.
    fn descending_first(self) -> bool;
}

/// The column a table is ordered by and which way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sorting<C> {
    column: C,
    descending: bool,
}

impl<C: Sortable + Default> Default for Sorting<C> {
    fn default() -> Self {
        Self::new(C::default())
    }
}

impl<C: Sortable> Sorting<C> {
    pub fn new(column: C) -> Self {
        Self {
            column,
            descending: column.descending_first(),
        }
    }

    /// Pressing the column in use turns the order round, and pressing another starts it in its own
    /// first direction.
    pub fn press(&mut self, column: C) {
        if self.column == column {
            self.descending = !self.descending;
        } else {
            *self = Self::new(column);
        }
    }

    /// Which way `column` is ordered, or `None` when the table is ordered by another.
    pub fn direction(self, column: C) -> Option<bool> {
        (self.column == column).then_some(self.descending)
    }

    /// Puts `rows` in order, with `compare` giving the ascending order of two rows by a column. Rows
    /// that compare equal keep the order they had.
    pub fn sort<T>(self, rows: &mut [T], compare: impl Fn(C, &T, &T) -> Ordering) {
        rows.sort_by(|a, b| {
            let order = compare(self.column, a, b);
            if self.descending {
                order.reverse()
            } else {
                order
            }
        });
    }
}

/// The side of the square a column heading's arrow is drawn in.
const ARROW: f32 = 12.0;
/// The side of the square a section heading's arrow is drawn in, sized for its larger text.
const FOLD_ARROW: f32 = 20.0;

/// The way a small triangle points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Arrow {
    Up,
    Down,
    Right,
}

impl Arrow {
    fn view<'a, Message: 'a>(self, side: f32, role: &'static str) -> Element<'a, Message> {
        canvas::Canvas::new(ArrowIcon { arrow: self, role })
            .width(side)
            .height(side)
            .into()
    }
}

struct ArrowIcon {
    arrow: Arrow,
    role: &'static str,
}

impl<Message> canvas::Program<Message, Theme> for ArrowIcon {
    type State = ();

    fn draw(
        &self,
        (): &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (width, height) = (frame.width(), frame.height());
        let [first, second, tip] = match self.arrow {
            Arrow::Down => [(0.15, 0.3), (0.85, 0.3), (0.5, 0.75)],
            Arrow::Up => [(0.15, 0.7), (0.85, 0.7), (0.5, 0.25)],
            Arrow::Right => [(0.3, 0.15), (0.3, 0.85), (0.75, 0.5)],
        }
        .map(|(x, y)| Point::new(width * x, height * y));
        let triangle = Path::new(|path| {
            path.move_to(first);
            path.line_to(second);
            path.line_to(tip);
            path.close();
        });
        frame.fill(
            &triangle,
            match self.role {
                "section_heading" => styles::section_heading(theme),
                _ => styles::table_heading(theme),
            }
            .color
            .unwrap_or(theme.palette().text),
        );
        vec![frame.into_geometry()]
    }
}

/// A heading that folds the section under it away or opens it again, with a triangle that points
/// at the content when it is open and to the side when it is folded.
pub fn fold_header<'a, Message: Clone + 'a>(
    label: &'a str,
    open: bool,
    press: Message,
) -> Element<'a, Message> {
    let arrow = if open { Arrow::Down } else { Arrow::Right };
    button(
        row![
            arrow.view(FOLD_ARROW, "section_heading"),
            text(label).size(28).style(styles::section_heading)
        ]
        .spacing(10)
        .align_y(iced::Center),
    )
    .on_press(press)
    .style(styles::button_text)
    .into()
}

/// A column heading that orders its table when pressed. It carries an arrow while its column is the
/// one in use, and keeps the room for one when it is not, so the headings do not shift.
pub fn sort_header<'a, Message: Clone + 'a>(
    label: &'a str,
    direction: Option<bool>,
    press: Message,
) -> Element<'a, Message> {
    let arrow: Element<'a, Message> = match direction {
        Some(true) => Arrow::Down.view(ARROW, "table_heading"),
        Some(false) => Arrow::Up.view(ARROW, "table_heading"),
        None => space().width(ARROW).height(ARROW).into(),
    };
    button(
        row![text(label).style(styles::table_heading), arrow]
            .spacing(6)
            .align_y(iced::Center),
    )
    .on_press(press)
    .style(styles::button_text)
    .into()
}

/// What pressing the control labelled `target` in `view` sends, found by drawing the view as a
/// window would and clicking where the label is.
#[cfg(test)]
pub fn pressed<'a, Message: 'a>(
    view: impl Into<Element<'a, Message>>,
    target: &str,
) -> Vec<Message> {
    let mut simulator = iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1280.0, 800.0),
        view,
    );
    simulator.click(target).unwrap();
    simulator.into_messages().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Column {
        Name,
        Size,
    }

    impl Sortable for Column {
        fn descending_first(self) -> bool {
            self == Self::Size
        }
    }

    fn by_column(column: Column, a: &(&str, u32), b: &(&str, u32)) -> Ordering {
        match column {
            Column::Name => a.0.cmp(b.0),
            Column::Size => a.1.cmp(&b.1),
        }
    }

    #[test]
    fn a_column_starts_in_its_own_direction_and_turns_round_when_pressed_again() {
        let mut sorting = Sorting::new(Column::Name);
        assert_eq!(sorting.direction(Column::Name), Some(false));
        assert_eq!(sorting.direction(Column::Size), None);
        sorting.press(Column::Size);
        assert_eq!(sorting.direction(Column::Size), Some(true));
        sorting.press(Column::Size);
        assert_eq!(sorting.direction(Column::Size), Some(false));
        sorting.press(Column::Name);
        assert_eq!(sorting.direction(Column::Name), Some(false));
    }

    #[test]
    fn rows_are_put_in_the_order_the_column_and_direction_say() {
        let mut rows = [("b", 1), ("c", 3), ("a", 2)];
        Sorting::new(Column::Name).sort(&mut rows, by_column);
        assert_eq!(rows, [("a", 2), ("b", 1), ("c", 3)]);
        Sorting::new(Column::Size).sort(&mut rows, by_column);
        assert_eq!(rows, [("c", 3), ("a", 2), ("b", 1)]);
        let mut backwards = Sorting::new(Column::Name);
        backwards.press(Column::Name);
        backwards.sort(&mut rows, by_column);
        assert_eq!(rows, [("c", 3), ("b", 1), ("a", 2)]);
    }

    #[test]
    fn rows_that_tie_keep_the_order_they_had() {
        let mut rows = [("x", 1), ("a", 1), ("m", 1)];
        Sorting::new(Column::Size).sort(&mut rows, by_column);
        assert_eq!(rows, [("x", 1), ("a", 1), ("m", 1)]);
    }

    #[test]
    fn as_many_cells_as_hold_the_least_width_share_a_row() {
        assert_eq!(columns(1334.0, 120.0, 8.0), 10);
        assert_eq!(columns(976.0, 120.0, 8.0), 7);
        assert_eq!(columns(248.0, 120.0, 8.0), 2);
        assert_eq!(columns(247.0, 120.0, 8.0), 1);
    }

    #[test]
    fn a_row_always_holds_one_cell() {
        assert_eq!(columns(40.0, 120.0, 8.0), 1);
        assert_eq!(columns(0.0, 120.0, 8.0), 1);
    }
}
