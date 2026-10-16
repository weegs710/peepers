use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, LineCap, LineJoin, Path, Stroke};
use iced::{Color, Element, Fill, Point, Rectangle, Renderer, Size, Theme};

use crate::history::CAPACITY;

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

/// A scrolling line graph; the newest value is at the right edge.
#[derive(Debug)]
pub struct Graph {
    scale: Scale,
    lines: Vec<(Vec<Option<f32>>, Tone)>,
    /// How many values fit across the width.
    span: usize,
    /// Where to draw a vertical cursor, as a fraction of the width.
    marker: Option<f32>,
}

impl Graph {
    pub fn new(scale: Scale) -> Self {
        Self {
            scale,
            lines: Vec::new(),
            span: CAPACITY,
            marker: None,
        }
    }

    pub fn span(mut self, span: usize) -> Self {
        self.span = span.max(2);
        self
    }

    pub fn marker(mut self, fraction: f32) -> Self {
        self.marker = Some(fraction.clamp(0.0, 1.0));
        self
    }

    pub fn line(mut self, values: Vec<f32>, tone: Tone) -> Self {
        self.lines
            .push((values.into_iter().map(Some).collect(), tone));
        self
    }

    /// Missing readings leave gaps, keeping the time axis without inventing zeroes.
    pub fn optional_line(mut self, values: Vec<Option<f32>>, tone: Tone) -> Self {
        self.lines.push((values, tone));
        self
    }

    pub fn view<'a, Message: 'a>(self) -> Element<'a, Message> {
        iced::widget::canvas(self).width(Fill).height(Fill).into()
    }

    fn ceiling(&self) -> f32 {
        match self.scale {
            Scale::Fixed(ceiling) => ceiling,
            Scale::Fit { floor } => self
                .lines
                .iter()
                .flat_map(|(values, _)| values)
                .flatten()
                .fold(floor, |peak, &value| peak.max(value)),
        }
    }
}

impl<Message> canvas::Program<Message> for Graph {
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
        let (width, height) = (bounds.width, bounds.height);
        let grid = Stroke::default().with_width(1.0).with_color(
            theme
                .extended_palette()
                .background
                .strong
                .color
                .scale_alpha(0.6),
        );
        for quarter in 1..4 {
            let y = height * along(quarter, 4);
            frame.stroke(&Path::line(Point::new(0.0, y), Point::new(width, y)), grid);
        }

        let ceiling = self.ceiling().max(f32::EPSILON);
        for (values, tone) in &self.lines {
            let color = tone.color(theme);
            for points in segments(values, self.span, ceiling, bounds.size()) {
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
                frame.fill(&area, color.scale_alpha(0.18));
                glow(&mut frame, &points, color, height);
                frame.stroke(&outline, round(1.5, lighten(color, 0.35)));
            }
        }
        if let Some(fraction) = self.marker {
            let x = width * fraction;
            let color = theme.extended_palette().background.base.text;
            frame.stroke(
                &Path::line(Point::new(x, 0.0), Point::new(x, height)),
                Stroke::default().with_width(1.0).with_color(color),
            );
        }
        vec![frame.into_geometry()]
    }
}

/// Separate paths at missing samples while retaining their positions on the time axis.
fn segments(values: &[Option<f32>], span: usize, ceiling: f32, size: Size) -> Vec<Vec<Point>> {
    let newest = values.len().saturating_sub(1);
    let mut segments = Vec::new();
    let mut segment = Vec::new();
    for (index, value) in values.iter().enumerate() {
        if let Some(value) = value.filter(|value| value.is_finite()) {
            segment.push(Point::new(
                size.width * (1.0 - along(newest - index, span - 1)),
                size.height * (1.0 - (value / ceiling).clamp(0.0, 1.0)),
            ));
        } else if !segment.is_empty() {
            segments.push(std::mem::take(&mut segment));
        }
    }
    if !segment.is_empty() {
        segments.push(segment);
    }
    segments
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
    let reach = (height * REACH).clamp(2.0, REACH_MAX) / REACH_MAX;
    for (full, total) in LAYERS {
        let radius = full * reach;
        if radius < RADIUS_MIN {
            continue;
        }
        let spacing = (radius * SPACING).max(SPACING_MIN);
        // Overlapping translucent stamps add up, so each is only the share that sums to `total`.
        let stamp = 1.0 - (1.0 - total).powf(spacing / (2.0 * radius));
        let mut disc = |at: Point| frame.fill(&Path::circle(at, radius), color.scale_alpha(stamp));
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
        assert!(
            segments(
                &[None, Some(f32::NAN), None],
                4,
                100.0,
                Size::new(300.0, 100.0)
            )
            .is_empty()
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
}
