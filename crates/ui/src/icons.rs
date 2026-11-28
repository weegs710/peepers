use crate::theme::{Element, Theme};
use iced::widget::canvas::{self, Frame, Geometry, LineCap, LineJoin, Path, Stroke};
use iced::{Point, Rectangle, Renderer, mouse};

use crate::Page;

use self::Shape::{Line, Outline};

/// The side of an icon on screen.
pub const SIZE: f32 = 18.0;

/// The side of the grid the shapes are laid out on.
const GRID: f32 = 24.0;
const STROKE: f32 = 2.0;
const ARC_STEPS: u8 = 24;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Shape {
    /// An open line through the points.
    Line(&'static [(f32, f32)]),
    /// A line through the points that comes back to the first.
    Outline(&'static [(f32, f32)]),
    Ring {
        x: f32,
        y: f32,
        radius: f32,
    },
    Dot {
        x: f32,
        y: f32,
        radius: f32,
    },
    /// Part of a circle, between two angles in degrees turning clockwise from the right.
    Arc {
        x: f32,
        y: f32,
        radius: f32,
        from: f32,
        to: f32,
    },
}

const fn ring(x: f32, y: f32, radius: f32) -> Shape {
    Shape::Ring { x, y, radius }
}

const fn dot(x: f32, y: f32, radius: f32) -> Shape {
    Shape::Dot { x, y, radius }
}

const fn arc(x: f32, y: f32, radius: f32, from: f32, to: f32) -> Shape {
    Shape::Arc {
        x,
        y,
        radius,
        from,
        to,
    }
}

const PROCESSES: &[Shape] = &[
    dot(4.0, 6.0, 1.5),
    Line(&[(9.0, 6.0), (20.0, 6.0)]),
    dot(4.0, 12.0, 1.5),
    Line(&[(9.0, 12.0), (20.0, 12.0)]),
    dot(4.0, 18.0, 1.5),
    Line(&[(9.0, 18.0), (20.0, 18.0)]),
];

const PERFORMANCE: &[Shape] = &[
    Line(&[(3.0, 3.0), (3.0, 21.0), (21.0, 21.0)]),
    Line(&[(7.0, 15.0), (11.0, 10.0), (14.0, 13.0), (20.0, 6.0)]),
];

const PRESSURE: &[Shape] = &[
    arc(12.0, 13.0, 9.0, 135.0, 405.0),
    Line(&[(12.0, 13.0), (16.5, 8.5)]),
    dot(12.0, 13.0, 1.6),
];

const POWER: &[Shape] = &[Outline(&[
    (13.0, 2.0),
    (5.0, 13.0),
    (11.0, 13.0),
    (10.0, 22.0),
    (19.0, 10.0),
    (13.0, 10.0),
])];

const FLIGHT: &[Shape] = &[
    Outline(&[(3.0, 7.0), (15.0, 7.0), (15.0, 17.0), (3.0, 17.0)]),
    Outline(&[(15.0, 10.5), (21.0, 7.5), (21.0, 16.5), (15.0, 13.5)]),
    dot(9.0, 12.0, 2.2),
];

const BENCHMARKS: &[Shape] = &[
    ring(12.0, 14.0, 8.0),
    Line(&[(12.0, 14.0), (12.0, 9.0)]),
    Line(&[(9.0, 2.0), (15.0, 2.0)]),
];

const CONNECTIONS: &[Shape] = &[
    Outline(&[(9.0, 3.0), (15.0, 3.0), (15.0, 8.0), (9.0, 8.0)]),
    Line(&[(12.0, 8.0), (12.0, 16.0)]),
    Line(&[(5.0, 16.0), (5.0, 12.0), (19.0, 12.0), (19.0, 16.0)]),
    Outline(&[(2.0, 16.0), (8.0, 16.0), (8.0, 21.0), (2.0, 21.0)]),
    Outline(&[(9.0, 16.0), (15.0, 16.0), (15.0, 21.0), (9.0, 21.0)]),
    Outline(&[(16.0, 16.0), (22.0, 16.0), (22.0, 21.0), (16.0, 21.0)]),
];

const DRIVERS: &[Shape] = &[
    Outline(&[(7.0, 7.0), (17.0, 7.0), (17.0, 17.0), (7.0, 17.0)]),
    Line(&[(10.0, 3.0), (10.0, 7.0)]),
    Line(&[(14.0, 3.0), (14.0, 7.0)]),
    Line(&[(10.0, 17.0), (10.0, 21.0)]),
    Line(&[(14.0, 17.0), (14.0, 21.0)]),
    Line(&[(3.0, 10.0), (7.0, 10.0)]),
    Line(&[(3.0, 14.0), (7.0, 14.0)]),
    Line(&[(17.0, 10.0), (21.0, 10.0)]),
    Line(&[(17.0, 14.0), (21.0, 14.0)]),
];

const DISK_SPACE: &[Shape] = &[
    ring(12.0, 12.0, 9.0),
    Line(&[(12.0, 3.0), (12.0, 12.0), (19.8, 16.5)]),
];

const APPS: &[Shape] = &[
    Outline(&[(4.0, 4.0), (10.0, 4.0), (10.0, 10.0), (4.0, 10.0)]),
    Outline(&[(14.0, 4.0), (20.0, 4.0), (20.0, 10.0), (14.0, 10.0)]),
    Outline(&[(4.0, 14.0), (10.0, 14.0), (10.0, 20.0), (4.0, 20.0)]),
    Outline(&[(14.0, 14.0), (20.0, 14.0), (20.0, 20.0), (14.0, 20.0)]),
];

const INFO: &[Shape] = &[
    ring(12.0, 12.0, 9.0),
    dot(12.0, 7.5, 1.2),
    Line(&[(12.0, 11.0), (12.0, 17.0)]),
];

const SERVICES: &[Shape] = &[
    Outline(&[(4.0, 4.0), (20.0, 4.0), (20.0, 10.0), (4.0, 10.0)]),
    Outline(&[(4.0, 14.0), (20.0, 14.0), (20.0, 20.0), (4.0, 20.0)]),
    dot(8.0, 7.0, 1.0),
    dot(8.0, 17.0, 1.0),
];

const STARTUP: &[Shape] = &[
    arc(12.0, 13.0, 8.0, -50.0, 230.0),
    Line(&[(12.0, 3.0), (12.0, 11.0)]),
];

const USERS: &[Shape] = &[ring(12.0, 8.0, 4.0), arc(12.0, 21.0, 8.0, 180.0, 360.0)];

const SETTINGS: &[Shape] = &[
    Line(&[(3.0, 6.0), (5.5, 6.0)]),
    Line(&[(10.5, 6.0), (21.0, 6.0)]),
    dot(8.0, 6.0, 2.5),
    Line(&[(3.0, 12.0), (13.5, 12.0)]),
    Line(&[(18.5, 12.0), (21.0, 12.0)]),
    dot(16.0, 12.0, 2.5),
    Line(&[(3.0, 18.0), (7.5, 18.0)]),
    Line(&[(12.5, 18.0), (21.0, 18.0)]),
    dot(10.0, 18.0, 2.5),
];

/// What a page's icon is drawn from.
fn shapes(page: Page) -> &'static [Shape] {
    match page {
        Page::Processes => PROCESSES,
        Page::Performance => PERFORMANCE,
        Page::Pressure => PRESSURE,
        Page::Power => POWER,
        Page::Flight => FLIGHT,
        Page::Benchmarks => BENCHMARKS,
        Page::Connections => CONNECTIONS,
        Page::Drivers => DRIVERS,
        Page::DiskSpace => DISK_SPACE,
        Page::Apps => APPS,
        Page::Info => INFO,
        Page::Services => SERVICES,
        Page::Startup => STARTUP,
        Page::Users => USERS,
        Page::Settings => SETTINGS,
    }
}

/// The points of an arc, close enough together to read as a curve.
fn arc_points(x: f32, y: f32, radius: f32, from: f32, to: f32) -> Vec<(f32, f32)> {
    (0..=ARC_STEPS)
        .map(|step| {
            let angle = (from + (to - from) * f32::from(step) / f32::from(ARC_STEPS)).to_radians();
            (x + radius * angle.cos(), y + radius * angle.sin())
        })
        .collect()
}

/// A page's icon, in the colour of the label beside it.
pub fn page<'a, Message: 'a>(page: Page, selected: bool) -> Element<'a, Message> {
    canvas::Canvas::new(Icon { page, selected })
        .width(SIZE)
        .height(SIZE)
        .into()
}

struct Icon {
    page: Page,
    selected: bool,
}

impl<Message> canvas::Program<Message, Theme> for Icon {
    type State = ();

    fn draw(
        &self,
        (): &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let palette = theme.extended_palette();
        let color = if self.selected {
            theme.color("selected_icon", palette.primary.base.text)
        } else {
            theme.color("icon", palette.background.base.text)
        };
        let scale = bounds.width / GRID;
        let at = |(x, y): (f32, f32)| Point::new(x * scale, y * scale);
        let stroke = Stroke::default()
            .with_width(STROKE * scale)
            .with_color(color)
            .with_line_cap(LineCap::Round)
            .with_line_join(LineJoin::Round);
        let through = |points: &[(f32, f32)], closed: bool| {
            Path::new(|builder| {
                let mut points = points.iter().copied().map(at);
                if let Some(first) = points.next() {
                    builder.move_to(first);
                    points.for_each(|point| builder.line_to(point));
                }
                if closed {
                    builder.close();
                }
            })
        };

        let mut frame = Frame::new(renderer, bounds.size());
        for shape in shapes(self.page) {
            match *shape {
                Shape::Line(points) => frame.stroke(&through(points, false), stroke),
                Shape::Outline(points) => frame.stroke(&through(points, true), stroke),
                Shape::Ring { x, y, radius } => {
                    frame.stroke(&Path::circle(at((x, y)), radius * scale), stroke);
                }
                Shape::Dot { x, y, radius } => {
                    frame.fill(&Path::circle(at((x, y)), radius * scale), color);
                }
                Shape::Arc {
                    x,
                    y,
                    radius,
                    from,
                    to,
                } => frame.stroke(&through(&arc_points(x, y, radius, from, to), false), stroke),
            }
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where a shape's ink can reach, as the least and greatest of each coordinate.
    fn reach(shape: Shape) -> [f32; 4] {
        let points = match shape {
            Shape::Line(points) | Shape::Outline(points) => points.to_vec(),
            Shape::Ring { x, y, radius } | Shape::Dot { x, y, radius } => {
                vec![(x - radius, y - radius), (x + radius, y + radius)]
            }
            Shape::Arc {
                x,
                y,
                radius,
                from,
                to,
            } => arc_points(x, y, radius, from, to),
        };
        points.into_iter().fold(
            [f32::MAX, f32::MAX, f32::MIN, f32::MIN],
            |[left, top, right, bottom], (x, y)| {
                [left.min(x), top.min(y), right.max(x), bottom.max(y)]
            },
        )
    }

    #[test]
    fn every_icon_keeps_its_stroke_inside_the_grid() {
        let margin = STROKE / 2.0 - 0.01;
        for page in Page::ALL {
            for shape in shapes(page) {
                let [left, top, right, bottom] = reach(*shape);
                assert!(
                    left >= margin
                        && top >= margin
                        && right <= GRID - margin
                        && bottom <= GRID - margin,
                    "{page:?} reaches {left},{top} to {right},{bottom}"
                );
            }
        }
    }

    #[test]
    fn no_two_pages_share_an_icon() {
        for (at, page) in Page::ALL.into_iter().enumerate() {
            assert!(!shapes(page).is_empty(), "{page:?} has no icon");
            for other in &Page::ALL[at + 1..] {
                assert_ne!(shapes(page), shapes(*other), "{page:?} and {other:?}");
            }
        }
    }

    #[test]
    fn an_arc_runs_clockwise_from_the_first_angle_to_the_second() {
        let points = arc_points(10.0, 10.0, 5.0, 0.0, 90.0);
        let (first, last) = (points[0], points[points.len() - 1]);
        assert!((first.0 - 15.0).abs() < 1e-3 && (first.1 - 10.0).abs() < 1e-3);
        assert!((last.0 - 10.0).abs() < 1e-3 && (last.1 - 15.0).abs() < 1e-3);
    }
}
