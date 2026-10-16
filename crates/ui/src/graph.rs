use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::{Color, Element, Fill, Point, Rectangle, Renderer, Theme};

use crate::history::CAPACITY;

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
    lines: Vec<(Vec<f32>, Tone)>,
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
            let newest = values.len().saturating_sub(1);
            let points: Vec<Point> = values
                .iter()
                .enumerate()
                .map(|(index, &value)| {
                    Point::new(
                        width * (1.0 - along(newest - index, self.span - 1)),
                        height * (1.0 - (value / ceiling).clamp(0.0, 1.0)),
                    )
                })
                .collect();
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
            frame.stroke(
                &outline,
                Stroke::default().with_width(1.5).with_color(color),
            );
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
