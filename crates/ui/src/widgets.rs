use iced::widget::grid::Sizing;
use iced::widget::text::Wrapping;
use iced::widget::{Scrollable, column, container, grid, responsive, row, scrollable, space, text};
use iced::{Element, Fill, Length};

use peepers_core::telemetry::Vitals;

use crate::graph::Graph;

/// Space kept between a scrollbar and the content, so the bar never covers the last column.
const SCROLLBAR_GAP: f32 = 6.0;

/// A scrollable whose bar has a strip of its own instead of floating over the content.
pub fn scrolling<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> Scrollable<'a, Message> {
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
        text(label.into()).size(12).style(text::secondary),
        text(value.into()).size(18).wrapping(Wrapping::WordOrGlyph)
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
            text("●").style(if busy { text::success } else { text::secondary }),
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
            text(title.into()).size(26),
            space::horizontal(),
            text(subtitle.unwrap_or_default()).style(text::secondary)
        ],
        container(graph.view()).height(Fill).width(Fill),
        container(scrolling(facts).width(Fill)).max_height(available * FACTS_SHARE),
    ]
    .spacing(14)
    .padding(16)
    .height(Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

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
