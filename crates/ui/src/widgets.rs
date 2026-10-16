use iced::widget::{Scrollable, column, container, row, scrollable, space, text};
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

/// A caption above a value.
pub fn stat<'a, Message: 'a>(
    label: impl Into<String>,
    value: impl Into<String>,
) -> Element<'a, Message> {
    column![
        text(label.into()).size(12).style(text::secondary),
        text(value.into()).size(18)
    ]
    .spacing(2)
    .width(150)
    .into()
}

/// Captioned values flowing left to right and wrapping at the edge.
pub fn stats<'a, Message: 'a, L, V>(items: impl IntoIterator<Item = (L, V)>) -> Element<'a, Message>
where
    L: Into<String>,
    V: Into<String>,
{
    row(items.into_iter().map(|(label, value)| stat(label, value)))
        .spacing(20)
        .wrap()
        .vertical_spacing(14)
        .into()
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

/// Title and subtitle over a graph that takes the free height, with a block of facts below.
pub fn detail<'a, Message: 'a>(
    title: impl Into<String>,
    subtitle: Option<String>,
    graph: Graph,
    facts: Element<'a, Message>,
) -> Element<'a, Message> {
    column![
        row![
            text(title.into()).size(26),
            space::horizontal(),
            text(subtitle.unwrap_or_default()).style(text::secondary)
        ],
        container(graph.view())
            .height(Length::FillPortion(3))
            .width(Fill),
        facts,
    ]
    .spacing(14)
    .padding(16)
    .into()
}
