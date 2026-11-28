use crate::theme::styles;
use crate::theme::{Element, Theme};
use std::cell::Cell;
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::time::Duration;

use iced::alignment::Vertical;
use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, LineCap, LineJoin, Path, Stroke};
use iced::widget::{text, tooltip};
use iced::{Color, Fill, Point, Rectangle, Renderer, Size};

use crate::format;
use crate::history::CAPACITY;

/// A graph shorter than this carries no scale: the labels would crowd the line they describe.
const AXIS_MIN_HEIGHT: f32 = 150.0;
/// The height of a card's graph where the scale has room to show.
pub const LABELLED_HEIGHT: f32 = AXIS_MIN_HEIGHT + 10.0;
const AXIS_TEXT: f32 = 11.0;
const AXIS_INSET: f32 = 4.0;

/// How long the pointer rests on a graph before its tooltip shows, so one passing over does not
/// flash it.
const TOOLTIP_DELAY: Duration = Duration::from_millis(400);
/// The tooltip's rows: how wide the whole is, how tall and how large the text of each, and the
/// radius of the dot that gives a line's colour.
const LEGEND_WIDTH: f32 = 220.0;
const LEGEND_ROW: f32 = 24.0;
const LEGEND_TEXT: f32 = 16.0;
const LEGEND_DOT: f32 = 4.0;
/// The radius of the dot where the pointer's moment meets a line.
const MARK_DOT: f32 = 3.5;

/// How far a line's glow reaches, as a share of the graph's height and at most this many pixels, so
/// that the glow of a short graph does not swamp the line.
const REACH: f32 = 0.12;
const REACH_MAX: f32 = 14.0;
/// The glow is soft discs stamped along the line, which fade out evenly and cannot sprout the
/// shards that one very wide stroke leaves at a sharp turn. Each layer is a radius at full reach and
/// the strength it adds up to where its stamps overlap; the widest comes first.
const LAYERS: [(f32, f32); 3] = [(14.0, 0.105), (9.0, 0.11), (5.0, 0.155)];
/// Stamps sit this many radii apart, so each layer is the same few stamps deep along the line.
const SPACING: f32 = 0.5;
/// A layer narrower than this many pixels would be lost in the line itself, so it is skipped.
const RADIUS_MIN: f32 = 2.5;
/// The closest stamps ever sit, in pixels, so that a small graph does not pay for fine spacing it
/// cannot show.
const SPACING_MIN: f32 = 2.0;
const SIDES: usize = 8;

#[derive(Debug, Clone, Copy)]
pub enum Tone {
    Primary,
    Success,
    Warning,
}

impl Tone {
    fn color(self, theme: &Theme) -> Color {
        let palette = theme.extended_palette();
        match self {
            Self::Primary => palette.primary.strong.color,
            Self::Success => palette.success.strong.color,
            Self::Warning => palette.warning.strong.color,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Scale {
    Fixed(f32),
    /// Fits the largest value, never dropping below `floor` so an idle graph does not amplify noise.
    Fit {
        floor: f32,
    },
}

#[derive(Debug)]
struct Line {
    values: Vec<Option<f32>>,
    tone: Tone,
    key: &'static str,
    /// What the line shows, which the tooltip tells next to its newest value.
    name: Option<String>,
}

/// How a graph value is written. A closure rather than a function, since a unit such as the
/// temperature scale is only known at run time.
#[derive(Clone)]
struct Readout(Rc<dyn Fn(f32) -> String>);

impl fmt::Debug for Readout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Readout").finish_non_exhaustive()
    }
}

impl Line {
    /// The reading `back` samples before the newest, if there was one.
    fn at(&self, back: usize) -> Option<f32> {
        let index = self.values.len().checked_sub(back + 1)?;
        self.values[index].filter(|value| value.is_finite())
    }
}

/// A scrolling line graph; the newest value is at the right edge.
#[derive(Debug)]
pub struct Graph {
    scale: Scale,
    key: &'static str,
    instance: Option<String>,
    lines: Vec<Line>,
    /// How many values fit across the width.
    span: usize,
    /// Where to draw a vertical cursor, as a fraction of the width.
    marker: Option<f32>,
    /// How a value is written, in the tooltip and on the scale; a graph without one has neither
    /// scale labels nor a unit to give them.
    readout: Option<Readout>,
}

impl Graph {
    /// Stable key for the last line; its display name may contain arbitrary device text.
    pub fn series(mut self, key: &'static str) -> Self {
        if let Some(line) = self.lines.last_mut() {
            line.key = key;
        }
        self
    }

    pub fn new(scale: Scale) -> Self {
        Self {
            scale,
            key: "",
            instance: None,
            lines: Vec::new(),
            span: CAPACITY,
            marker: None,
            readout: None,
        }
    }

    /// Stable theme key, independent of translated or device-specific display labels.
    pub fn key(mut self, key: &'static str) -> Self {
        self.key = key;
        self
    }

    pub fn instance(mut self, instance: impl Into<String>) -> Self {
        self.instance = Some(instance.into());
        self
    }

    fn color(&self, theme: &Theme, part: &str, fallback: Color) -> Color {
        theme
            .graph_color(self.key, self.instance.as_deref(), None, part)
            .unwrap_or(fallback)
    }

    fn tooltip_style(&self, theme: &Theme) -> iced::widget::container::Style {
        let mut style = styles::container_bordered_box(theme);
        if let Some(color) = theme
            .graph_color(self.key, self.instance.as_deref(), None, "tooltip")
            .or_else(|| theme.override_color("tooltip"))
        {
            style.background = Some(color.into());
        }
        if let Some(color) = theme
            .graph_color(self.key, self.instance.as_deref(), None, "legend")
            .or_else(|| theme.override_color("tooltip_text"))
        {
            style.text_color = Some(color);
        }
        style
    }

    fn line_color(&self, theme: &Theme, line: &Line, part: &str) -> Color {
        let lookup =
            |part| theme.graph_color(self.key, self.instance.as_deref(), Some(line.key), part);
        let role = match line.tone {
            Tone::Primary => "primary",
            Tone::Success => "success",
            Tone::Warning => "warning",
        };
        let explicit = theme.graph_colors(
            self.key,
            self.instance.as_deref(),
            Some(line.key),
            &["color", role],
        );
        let base = explicit.unwrap_or_else(|| line.tone.color(theme));
        lookup(part)
            .or_else(|| {
                if part == "point" {
                    lookup("stroke")
                } else {
                    None
                }
            })
            .unwrap_or_else(|| match part {
                "fill" => base.scale_alpha(0.18),
                "stroke" | "point" if explicit.is_none() => lighten(base, 0.35),
                _ => base,
            })
    }

    pub fn span(mut self, span: usize) -> Self {
        self.span = span.max(2);
        self
    }

    pub fn marker(mut self, fraction: f32) -> Self {
        self.marker = Some(fraction.clamp(0.0, 1.0));
        self
    }

    pub fn line(self, values: Vec<f32>, tone: Tone) -> Self {
        self.optional_line(values.into_iter().map(Some).collect(), tone)
    }

    /// Missing readings leave gaps, keeping the time axis without inventing zeroes.
    pub fn optional_line(mut self, values: Vec<Option<f32>>, tone: Tone) -> Self {
        self.lines.push(Line {
            values,
            tone,
            key: "",
            name: None,
        });
        self
    }

    /// Names the line added last. Hovering a graph with named lines lists them with their newest
    /// values, so a graph with two lines of one colour family says which is which.
    pub fn named(mut self, name: impl Into<String>) -> Self {
        if let Some(line) = self.lines.last_mut() {
            line.name = Some(name.into());
        }
        self
    }

    /// How a value is written, which also labels the scale on a graph tall enough to carry it.
    pub fn readout(mut self, readout: impl Fn(f32) -> String + 'static) -> Self {
        self.readout = Some(Readout(Rc::new(readout)));
        self
    }

    pub fn view<'a, Message: 'a>(self) -> Element<'a, Message> {
        let named = self.lines.iter().filter(|line| line.name.is_some()).count();
        let graph = Rc::new(self);
        let hover = Hover::default();
        let plot = iced::widget::canvas(Plot {
            graph: Rc::clone(&graph),
            hover: Rc::clone(&hover),
        })
        .width(Fill)
        .height(Fill);
        if named == 0 {
            return plot.into();
        }
        let tooltip_graph = Rc::clone(&graph);
        let legend = iced::widget::canvas(Legend { graph, hover })
            .width(LEGEND_WIDTH)
            .height(level(named as u64) * LEGEND_ROW);
        tooltip(plot, legend, tooltip::Position::FollowCursor)
            .delay(TOOLTIP_DELAY)
            .padding(8)
            .style(move |theme: &Theme| tooltip_graph.tooltip_style(theme))
            .into()
    }

    /// One row per named line: its colour, its name and its value `fraction` of the way across,
    /// or its newest value when the pointer is not on the graph.
    fn entries(&self, fraction: Option<f32>) -> Vec<(Tone, String, String)> {
        let back = fraction.map_or(0, |fraction| steps_back(self.span, fraction));
        self.lines
            .iter()
            .filter_map(|line| {
                Some((
                    line.tone,
                    line.name.clone()?,
                    line.at(back)
                        .map_or_else(format::unavailable, |value| self.write(value)),
                ))
            })
            .collect()
    }

    fn write(&self, value: f32) -> String {
        self.readout
            .as_ref()
            .map_or_else(|| plain(value), |readout| (readout.0)(value))
    }

    fn ceiling(&self) -> f32 {
        match self.scale {
            Scale::Fixed(ceiling) => ceiling,
            Scale::Fit { floor } => self
                .lines
                .iter()
                .flat_map(|line| &line.values)
                .flatten()
                .fold(floor, |peak, &value| peak.max(value)),
        }
    }
}

/// How far across a graph the pointer rests, from 0 at the left edge to 1 at the right. The graph
/// that sees the pointer writes it and the tooltip that reports the values there reads it.
type Hover = Rc<Cell<Option<f32>>>;

/// What a graph keeps between redraws: its finished picture, and what the picture was drawn from.
#[derive(Default)]
pub struct Drawn {
    picture: canvas::Cache,
    from: Cell<Option<u64>>,
}

/// The graph itself, drawn with a mark on the moment the pointer is over.
struct Plot {
    graph: Rc<Graph>,
    hover: Hover,
}

impl<Message> canvas::Program<Message, Theme> for Plot {
    type State = Drawn;

    /// The glow is hundreds of discs to tessellate, and a window is redrawn for any pointer move
    /// over it, so the picture is only made again when what it shows has changed.
    fn draw(
        &self,
        drawn: &Drawn,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let fingerprint = self.graph.fingerprint(theme);
        if drawn.from.replace(Some(fingerprint)) != Some(fingerprint) {
            drawn.picture.clear();
        }
        let mut layers = vec![drawn.picture.draw(renderer, bounds.size(), |frame| {
            self.graph.paint(frame, theme);
        })];
        // The tooltip is drawn right after this, and it is the one that needs the pointer.
        let fraction = cursor
            .position_in(bounds)
            .map(|pointer| pointer.x / bounds.width);
        self.hover.set(fraction);
        if let Some(fraction) = fraction {
            let mut frame = Frame::new(renderer, bounds.size());
            self.graph.mark(&mut frame, theme, fraction);
            layers.push(frame.into_geometry());
        }
        layers
    }
}

/// The tooltip's rows, each line's value at the moment the pointer is over.
struct Legend {
    graph: Rc<Graph>,
    hover: Hover,
}

impl<Message> canvas::Program<Message, Theme> for Legend {
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
        let right = frame.width();
        let color = self
            .graph
            .tooltip_style(theme)
            .text_color
            .unwrap_or(theme.extended_palette().background.weakest.text);
        let entries = self.graph.entries(self.hover.get());
        for (row, ((_, name, value), line)) in entries
            .into_iter()
            .zip(self.graph.lines.iter().filter(|line| line.name.is_some()))
            .enumerate()
        {
            let y = (level(row as u64) + 0.5) * LEGEND_ROW;
            let write = |content: String, x: f32, align_x: text::Alignment| canvas::Text {
                content,
                position: Point::new(x, y),
                color,
                size: LEGEND_TEXT.into(),
                align_x,
                align_y: Vertical::Center,
                ..canvas::Text::default()
            };
            frame.fill(
                &Path::circle(Point::new(LEGEND_DOT, y), LEGEND_DOT),
                self.graph.line_color(theme, line, "stroke"),
            );
            frame.fill_text(write(name, LEGEND_DOT * 3.0, text::Alignment::Left));
            frame.fill_text(write(value, right, text::Alignment::Right));
        }
        vec![frame.into_geometry()]
    }
}

impl Graph {
    /// Everything but the size, which the cache already tells apart, that changes the picture.
    fn fingerprint(&self, theme: &Theme) -> u64 {
        let mut hasher = DefaultHasher::new();
        // The colours and not the name, because noctalia's theme keeps its name when its colours change.
        let palette = theme.palette();
        for colour in [
            palette.background,
            palette.text,
            palette.primary,
            palette.success,
            palette.warning,
            palette.danger,
        ] {
            colour.into_rgba8().hash(&mut hasher);
        }
        let extended = theme.extended_palette();
        for (part, fallback) in [
            ("background", Color::TRANSPARENT),
            ("grid", extended.background.strong.color.scale_alpha(0.6)),
            ("axis", extended.background.base.text.scale_alpha(0.55)),
            ("marker", extended.background.base.text),
            ("crosshair", extended.background.base.text.scale_alpha(0.4)),
        ] {
            self.color(theme, part, fallback)
                .into_rgba8()
                .hash(&mut hasher);
        }
        for line in &self.lines {
            for part in ["fill", "glow", "stroke", "point"] {
                self.line_color(theme, line, part)
                    .into_rgba8()
                    .hash(&mut hasher);
            }
        }
        match self.scale {
            Scale::Fixed(ceiling) => (0_u8, ceiling.to_bits()),
            Scale::Fit { floor } => (1, floor.to_bits()),
        }
        .hash(&mut hasher);
        self.span.hash(&mut hasher);
        self.marker.map(f32::to_bits).hash(&mut hasher);
        if self.readout.is_some() {
            let ceiling = self.ceiling();
            for quarter in 0..4 {
                self.write(ceiling * (1.0 - along(quarter, 4)))
                    .hash(&mut hasher);
            }
        }
        for line in &self.lines {
            std::mem::discriminant(&line.tone).hash(&mut hasher);
            for value in &line.values {
                value.map(f32::to_bits).hash(&mut hasher);
            }
        }
        hasher.finish()
    }

    /// A line down the moment `fraction` of the way across, with a dot where each line was read.
    fn mark(&self, frame: &mut Frame, theme: &Theme, fraction: f32) {
        let back = steps_back(self.span, fraction);
        let ceiling = self.ceiling().max(f32::EPSILON);
        let dots: Vec<(Point, Color)> = self
            .lines
            .iter()
            .filter_map(|line| {
                let at = place(back, self.span, line.at(back)?, ceiling, frame.size());
                Some((at, self.line_color(theme, line, "point")))
            })
            .collect();
        let Some((first, _)) = dots.first() else {
            return;
        };
        let color = self.color(
            theme,
            "crosshair",
            theme
                .extended_palette()
                .background
                .base
                .text
                .scale_alpha(0.4),
        );
        frame.stroke(
            &Path::line(
                Point::new(first.x, 0.0),
                Point::new(first.x, frame.height()),
            ),
            Stroke::default().with_width(1.0).with_color(color),
        );
        for (at, color) in dots {
            frame.fill(&Path::circle(at, MARK_DOT), color);
        }
    }

    fn paint(&self, frame: &mut Frame, theme: &Theme) {
        let (width, height) = (frame.width(), frame.height());
        frame.fill_rectangle(
            Point::ORIGIN,
            frame.size(),
            self.color(theme, "background", Color::TRANSPARENT),
        );
        let grid = Stroke::default().with_width(1.0).with_color(
            self.color(
                theme,
                "grid",
                theme
                    .extended_palette()
                    .background
                    .strong
                    .color
                    .scale_alpha(0.6),
            ),
        );
        for quarter in 1..4 {
            let y = height * along(quarter, 4);
            frame.stroke(&Path::line(Point::new(0.0, y), Point::new(width, y)), grid);
        }

        let ceiling = self.ceiling().max(f32::EPSILON);
        if self.readout.is_some() && height >= AXIS_MIN_HEIGHT {
            scale(
                frame,
                self.color(
                    theme,
                    "axis",
                    theme
                        .extended_palette()
                        .background
                        .base
                        .text
                        .scale_alpha(0.55),
                ),
                height,
                |share| self.write(ceiling * share),
            );
        }
        for line in &self.lines {
            let color = self.line_color(theme, line, "glow");
            let fill = self.line_color(theme, line, "fill");
            let stroke = self.line_color(theme, line, "stroke");
            for points in segments(&line.values, self.span, ceiling, frame.size()) {
                let (Some(first), Some(last)) = (points.first(), points.last()) else {
                    continue;
                };
                let outline = Path::new(|builder| {
                    builder.move_to(*first);
                    for &point in &points[1..] {
                        builder.line_to(point);
                    }
                });
                let area = Path::new(|builder| {
                    builder.move_to(Point::new(first.x, height));
                    for &point in &points {
                        builder.line_to(point);
                    }
                    builder.line_to(Point::new(last.x, height));
                    builder.close();
                });
                frame.fill(&area, fill);
                glow(frame, &points, color, height);
                frame.stroke(&outline, round(1.5, stroke));
            }
        }
        if let Some(fraction) = self.marker {
            let x = width * fraction;
            let color = self.color(
                theme,
                "marker",
                theme.extended_palette().background.base.text,
            );
            frame.stroke(
                &Path::line(Point::new(x, 0.0), Point::new(x, height)),
                Stroke::default().with_width(1.0).with_color(color),
            );
        }
    }
}

/// Writes the value each gridline stands for, and the ceiling at the top, down the left edge.
fn scale(frame: &mut Frame, color: Color, height: f32, label: impl Fn(f32) -> String) {
    for quarter in 0..4 {
        let y = height * along(quarter, 4);
        let (position, align_y) = if quarter == 0 {
            (Point::new(AXIS_INSET, AXIS_INSET), Vertical::Top)
        } else {
            (Point::new(AXIS_INSET, y - AXIS_INSET), Vertical::Bottom)
        };
        frame.fill_text(canvas::Text {
            content: label(1.0 - along(quarter, 4)),
            position,
            color,
            size: AXIS_TEXT.into(),
            align_y,
            ..canvas::Text::default()
        });
    }
}

/// Separate paths at missing samples while retaining their positions on the time axis.
fn segments(values: &[Option<f32>], span: usize, ceiling: f32, size: Size) -> Vec<Vec<Point>> {
    let newest = values.len().saturating_sub(1);
    let mut segments = Vec::new();
    let mut segment = Vec::new();
    for (index, value) in values.iter().enumerate() {
        if let Some(value) = value.filter(|value| value.is_finite()) {
            segment.push(place(newest - index, span, value, ceiling, size));
        } else if !segment.is_empty() {
            segments.push(std::mem::take(&mut segment));
        }
    }
    if !segment.is_empty() {
        segments.push(segment);
    }
    segments
}

/// Where a reading `back` samples before the newest sits, the newest being at the right edge.
fn place(back: usize, span: usize, value: f32, ceiling: f32, size: Size) -> Point {
    Point::new(
        size.width * (1.0 - along(back, span - 1)),
        size.height * (1.0 - (value / ceiling).clamp(0.0, 1.0)),
    )
}

/// How many samples before the newest `fraction` of the way across a graph `span` samples wide is.
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "graph indices stay far below 2^24, and the count is rounded and never negative"
)]
fn steps_back(span: usize, fraction: f32) -> usize {
    ((1.0 - fraction.clamp(0.0, 1.0)) * (span - 1) as f32).round() as usize
}

fn plain(value: f32) -> String {
    format!("{value:.0}")
}

fn lighten(color: Color, amount: f32) -> Color {
    let toward_white = |channel: f32| channel + (1.0 - channel) * amount;
    Color {
        r: toward_white(color.r),
        g: toward_white(color.g),
        b: toward_white(color.b),
        a: color.a,
    }
}

fn glow(frame: &mut Frame, points: &[Point], color: Color, height: f32) {
    if color.a == 0.0 {
        return;
    }
    let reach = (height * REACH).clamp(2.0, REACH_MAX) / REACH_MAX;
    for (full, total) in LAYERS {
        let radius = full * reach;
        if radius < RADIUS_MIN {
            continue;
        }
        let spacing = (radius * SPACING).max(SPACING_MIN);
        // Overlapping translucent stamps add up, so each is only the share that sums to `total`.
        let stamp = 1.0 - (1.0 - total).powf(spacing / (2.0 * radius));
        let mut disc = |at: Point| frame.fill(&polygon(at, radius), color.scale_alpha(stamp));
        for pair in points.windows(2) {
            let length = (pair[1].x - pair[0].x).hypot(pair[1].y - pair[0].y);
            let stamps = (length / spacing).ceil().max(1.0);
            let mut done = 0.0;
            while done < stamps {
                let along = done / stamps;
                disc(Point::new(
                    pair[0].x + (pair[1].x - pair[0].x) * along,
                    pair[0].y + (pair[1].y - pair[0].y) * along,
                ));
                done += 1.0;
            }
        }
        if let Some(&last) = points.last() {
            disc(last);
        }
    }
}

/// A stamp of the glow: few sides are enough, since the overlap of many stamps hides them.
fn polygon(centre: Point, radius: f32) -> Path {
    Path::new(|builder| {
        for side in 0..SIDES {
            let angle = std::f32::consts::TAU * along(side, SIDES);
            let corner = Point::new(
                centre.x + radius * angle.cos(),
                centre.y + radius * angle.sin(),
            );
            if side == 0 {
                builder.move_to(corner);
            } else {
                builder.line_to(corner);
            }
        }
        builder.close();
    })
}

fn round(width: f32, color: Color) -> Stroke<'static> {
    Stroke::default()
        .with_width(width)
        .with_color(color)
        .with_line_cap(LineCap::Round)
        .with_line_join(LineJoin::Round)
}

#[expect(
    clippy::cast_precision_loss,
    reason = "graph indices stay far below 2^24"
)]
fn along(index: usize, span: usize) -> f32 {
    index as f32 / span as f32
}

/// A graph value that is a share, written as a percentage.
pub fn as_percent(value: f32) -> String {
    format!("{value:.0}%")
}

/// A graph value that is a rate in bytes per second.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a rate is never negative and graph values need display precision only"
)]
pub fn as_rate(value: f32) -> String {
    format::rate(value.max(0.0) as u64)
}

/// A graph value that is a clock speed in kilohertz.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a clock is never negative and graph values need display precision only"
)]
pub fn as_frequency(value: f32) -> String {
    format::frequency(value.max(0.0) as u64)
}

/// A counter or rate as a graph value.
#[expect(
    clippy::cast_precision_loss,
    reason = "graph values need display precision only"
)]
pub fn level(value: u64) -> f32 {
    value as f32
}

/// At most `buckets` values, each the peak of the stretch it stands for, so a spike survives.
pub fn decimate(values: &[f32], buckets: usize) -> Vec<f32> {
    if values.len() <= buckets {
        return values.to_vec();
    }
    values
        .chunks(values.len().div_ceil(buckets))
        .map(|chunk| chunk.iter().copied().fold(f32::MIN, f32::max))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu(values: Vec<f32>) -> Graph {
        Graph::new(Scale::Fixed(100.0)).line(values, Tone::Primary)
    }

    #[test]
    fn tooltips_inherit_panels_until_tooltip_or_graph_colors_override_them() {
        let graph = Graph::new(Scale::Fixed(100.0)).key("cpu");
        let panel = "[ui]\npanel = '#112233'\npanel_text = '#AABBCC'";
        let theme = crate::theme::parse(panel, "Panel").unwrap();
        let style = graph.tooltip_style(&theme);
        assert_eq!(
            style.background,
            Some("#112233".parse::<Color>().unwrap().into())
        );
        assert_eq!(style.text_color, Some("#AABBCC".parse().unwrap()));
        let theme = crate::theme::parse(
            &format!("{panel}\ntooltip = '#223344'\ntooltip_text = '#BBCCDD'"),
            "Tooltip",
        )
        .unwrap();
        let style = graph.tooltip_style(&theme);
        assert_eq!(
            style.background,
            Some("#223344".parse::<Color>().unwrap().into())
        );
        assert_eq!(style.text_color, Some("#BBCCDD".parse().unwrap()));
        let theme = crate::theme::parse(
            &format!("{panel}\n[widgets.container]\nbackground = '#334455'\ntext = '#CCDDEE'"),
            "Container",
        )
        .unwrap();
        let style = graph.tooltip_style(&theme);
        assert_eq!(
            style.background,
            Some("#334455".parse::<Color>().unwrap().into())
        );
        assert_eq!(style.text_color, Some("#CCDDEE".parse().unwrap()));
        let theme = crate::theme::parse(
            &format!("{panel}\n[graphs.cpu]\ntooltip = '#445566'\nlegend = '#DDEEFF'"),
            "Graph",
        )
        .unwrap();
        let style = graph.tooltip_style(&theme);
        assert_eq!(
            style.background,
            Some("#445566".parse::<Color>().unwrap().into())
        );
        assert_eq!(style.text_color, Some("#DDEEFF".parse().unwrap()));
    }

    #[test]
    fn the_same_picture_has_the_same_fingerprint() {
        let dark = Theme::Dark;
        assert_eq!(
            cpu(vec![1.0, 2.0]).fingerprint(&dark),
            cpu(vec![1.0, 2.0]).fingerprint(&dark)
        );
    }

    #[test]
    fn anything_that_changes_the_picture_changes_the_fingerprint() {
        let dark = Theme::Dark;
        let base = cpu(vec![1.0, 2.0]).fingerprint(&dark);
        assert_ne!(base, cpu(vec![1.0, 2.5]).fingerprint(&dark));
        assert_ne!(base, cpu(vec![1.0, 2.0, 3.0]).fingerprint(&dark));
        assert_ne!(base, cpu(vec![1.0, 2.0]).fingerprint(&Theme::Light));
        let mut recoloured = Theme::Dark.palette();
        recoloured.primary = iced::Color::WHITE;
        assert_ne!(
            cpu(vec![1.0, 2.0]).fingerprint(&Theme::custom("Same", Theme::Dark.palette())),
            cpu(vec![1.0, 2.0]).fingerprint(&Theme::custom("Same", recoloured))
        );
        assert_ne!(base, cpu(vec![1.0, 2.0]).marker(0.5).fingerprint(&dark));
        assert_ne!(base, cpu(vec![1.0, 2.0]).span(10).fingerprint(&dark));
        assert_ne!(
            base,
            cpu(vec![1.0, 2.0]).readout(as_percent).fingerprint(&dark)
        );
        assert_ne!(
            base,
            Graph::new(Scale::Fit { floor: 100.0 })
                .line(vec![1.0, 2.0], Tone::Primary)
                .fingerprint(&dark)
        );
        assert_ne!(
            base,
            Graph::new(Scale::Fixed(100.0))
                .line(vec![1.0, 2.0], Tone::Warning)
                .fingerprint(&dark)
        );
    }

    #[test]
    fn a_readout_that_writes_differently_changes_the_picture() {
        let dark = Theme::Dark;
        let in_unit = |unit: &'static str| {
            cpu(vec![1.0, 2.0]).readout(move |value| format!("{value:.0} {unit}"))
        };
        assert_eq!(
            in_unit("C").fingerprint(&dark),
            in_unit("C").fingerprint(&dark)
        );
        assert_ne!(
            in_unit("C").fingerprint(&dark),
            in_unit("F").fingerprint(&dark)
        );
    }

    #[test]
    fn a_gap_is_not_a_zero() {
        let dark = Theme::Dark;
        let gap =
            Graph::new(Scale::Fixed(100.0)).optional_line(vec![Some(1.0), None], Tone::Primary);
        assert_ne!(
            gap.fingerprint(&dark),
            cpu(vec![1.0, 0.0]).fingerprint(&dark)
        );
    }

    #[test]
    fn only_named_lines_are_listed_with_their_newest_value() {
        let graph = Graph::new(Scale::Fixed(100.0))
            .line(vec![10.0, 42.4], Tone::Primary)
            .named("Utilization")
            .line(vec![1.0], Tone::Warning)
            .optional_line(vec![Some(5.0), None], Tone::Success)
            .named("Video memory")
            .readout(as_percent);
        let entries: Vec<(String, String)> = graph
            .entries(None)
            .into_iter()
            .map(|(_, name, value)| (name, value))
            .collect();
        assert_eq!(
            entries,
            [
                ("Utilization".to_owned(), "42%".to_owned()),
                ("Video memory".to_owned(), "n/a".to_owned()),
            ]
        );
    }

    #[test]
    fn a_graph_with_no_names_has_no_tooltip() {
        let graph = Graph::new(Scale::Fixed(100.0)).line(vec![1.0], Tone::Primary);
        assert!(graph.entries(None).is_empty());
    }

    #[test]
    fn the_pointer_reads_the_value_where_it_rests() {
        let graph = Graph::new(Scale::Fixed(100.0))
            .span(5)
            .line(vec![10.0, 20.0, 30.0, 40.0, 50.0], Tone::Primary)
            .named("Utilization")
            .readout(as_percent);
        let at = |fraction| graph.entries(fraction)[0].2.clone();
        assert_eq!(at(None), "50%");
        assert_eq!(at(Some(1.0)), "50%");
        assert_eq!(at(Some(0.5)), "30%");
        assert_eq!(at(Some(0.0)), "10%");
    }

    #[test]
    fn where_there_was_no_reading_the_pointer_reads_unavailable() {
        let graph = Graph::new(Scale::Fixed(100.0))
            .span(5)
            .optional_line(vec![Some(10.0), None, Some(30.0)], Tone::Primary)
            .named("Utilization");
        let at = |fraction| graph.entries(Some(fraction))[0].2.clone();
        assert_eq!(at(1.0), "30");
        assert_eq!(at(0.75), "n/a");
        assert_eq!(at(0.5), "10");
        assert_eq!(at(0.0), "n/a");
    }

    #[test]
    fn a_pointer_position_counts_back_from_the_newest_sample() {
        assert_eq!(steps_back(5, 1.0), 0);
        assert_eq!(steps_back(5, 0.0), 4);
        assert_eq!(steps_back(5, 0.5), 2);
        assert_eq!(steps_back(5, 2.0), 0);
    }

    #[test]
    fn rates_read_in_bytes_and_never_below_zero() {
        assert_eq!(as_rate(1_048_576.0), "1.0 MiB/s");
        assert_eq!(as_rate(-5.0), "0 B/s");
    }

    #[test]
    fn clocks_read_in_gigahertz_from_kilohertz() {
        assert_eq!(as_frequency(3_200_000.0), "3.20 GHz");
        assert_eq!(as_frequency(-1.0), "0.00 GHz");
    }

    #[test]
    fn missing_readings_leave_gaps_without_shifting_time_or_hiding_real_zeroes() {
        let paths = segments(
            &[Some(25.0), None, Some(0.0), Some(100.0)],
            4,
            100.0,
            Size::new(300.0, 100.0),
        );
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], [Point::new(0.0, 75.0)]);
        assert_eq!(paths[1].len(), 2);
        for (actual, expected) in paths[1]
            .iter()
            .zip([Point::new(200.0, 100.0), Point::new(300.0, 0.0)])
        {
            assert!((actual.x - expected.x).abs() < 0.001);
            assert!((actual.y - expected.y).abs() < 0.001);
        }
        assert_eq!(
            segments(
                &[None, Some(f32::NAN), None],
                4,
                100.0,
                Size::new(300.0, 100.0)
            ),
            Vec::<Vec<Point>>::new()
        );
    }

    #[test]
    fn short_series_pass_through() {
        assert_eq!(decimate(&[1.0, 2.0, 3.0], 5), [1.0, 2.0, 3.0]);
    }

    #[test]
    fn decimation_keeps_the_peak_of_each_stretch() {
        let values = [1.0, 9.0, 2.0, 3.0, 8.0, 4.0];
        assert_eq!(decimate(&values, 3), [9.0, 3.0, 8.0]);
        assert!(decimate(&values, 4).len() <= 4);
    }
    #[test]
    fn line_only_edits_invalidate_cached_geometry_and_preserve_exact_colors() {
        let first =
            crate::theme::parse("[graphs.network.lines.receive]\ncolor = '#B66BFF'", "Same")
                .unwrap();
        let second =
            crate::theme::parse("[graphs.network.lines.receive]\ncolor = '#49E9FF'", "Same")
                .unwrap();
        let graph = Graph::new(Scale::Fixed(100.0))
            .key("network")
            .line(vec![1.0, 2.0], Tone::Success)
            .named("Receive")
            .series("receive");
        assert_eq!(first.palette(), second.palette());
        assert_ne!(graph.fingerprint(&first), graph.fingerprint(&second));
        assert_eq!(
            graph.line_color(&first, &graph.lines[0], "stroke"),
            "#B66BFF".parse().unwrap()
        );
        assert_eq!(
            graph.line_color(&first, &graph.lines[0], "point"),
            "#B66BFF".parse().unwrap()
        );
        let grid = crate::theme::parse("[graph]\ngrid = '#49E9FF'", "Same").unwrap();
        assert_ne!(graph.fingerprint(&Theme::Dark), graph.fingerprint(&grid));
    }
    #[test]
    fn derived_grid_edits_invalidate_cached_geometry() {
        let changed =
            crate::theme::parse("[extended.background.strong]\ncolor = '#49E9FF'", "Grid").unwrap();
        let graph = cpu(vec![10.0, 20.0]);
        assert_eq!(changed.palette(), Theme::Dark.palette());
        assert_ne!(graph.fingerprint(&changed), graph.fingerprint(&Theme::Dark));
    }
}
