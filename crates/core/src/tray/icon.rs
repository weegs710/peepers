use std::fmt;

use crate::telemetry::Vitals;

/// Icons travel as width, height and ARGB32 bytes in network order.
pub(super) type Pixmap = (i32, i32, Vec<u8>);

/// The size a tray that draws at 22 pixels shows the icon at without scaling it.
const SIZE: usize = 22;
/// Whole multiples of [`SIZE`] the icon is offered at, for a host that draws it larger.
const MAGNIFICATIONS: [usize; 2] = [1, 2];
const PANEL_HEIGHT: usize = 10;
const GAP: usize = SIZE - 2 * PANEL_HEIGHT;
const LABEL_X: usize = 1;
const PLOT_X: usize = 8;
const PLOT_TOP: usize = 1;
const PLOT_ROWS: u8 = 8;
/// How many readings the icon traces, one to a column.
const SAMPLES: usize = SIZE - PLOT_X - 1;
/// Opaque, so that the letters read the same on a light tray as on a dark one.
const BACKGROUND: [u8; 3] = [0x24, 0x24, 0x24];
const INK: [u8; 3] = [0xe0, 0xe0, 0xe0];

/// Opaque colours for one miniature graph. Load thresholds remain 60% and 85%.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphColors {
    pub background: [u8; 3],
    pub text: [u8; 3],
    pub low: [u8; 3],
    pub medium: [u8; 3],
    pub high: [u8; 3],
}

impl Default for GraphColors {
    fn default() -> Self {
        Self {
            background: BACKGROUND,
            text: INK,
            low: [0x4c, 0xaf, 0x50],
            medium: [0xff, 0xb3, 0x00],
            high: [0xe5, 0x39, 0x35],
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IconColors {
    pub cpu: GraphColors,
    pub memory: GraphColors,
}

const CPU_LABEL: [&str; 7] = [
    ".###.", "#...#", "#....", "#....", "#....", "#...#", ".###.",
];
const MEMORY_LABEL: [&str; 7] = [
    "#...#", "##.##", "#.#.#", "#...#", "#...#", "#...#", "#...#",
];

/// What the icon shows, as percentages.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Gauges {
    pub cpu: f32,
    pub memory: f32,
}

impl From<&Vitals> for Gauges {
    fn from(vitals: &Vitals) -> Self {
        Self {
            cpu: vitals.cpu.usage,
            memory: vitals.memory.used_percent(),
        }
    }
}

impl fmt::Display for Gauges {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "CPU {:.0}%   Memory {:.0}%",
            self.cpu, self.memory
        )
    }
}

/// The latest readings, oldest first, which the icon draws as a trace for each gauge.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct Trace {
    pub(super) colors: IconColors,
    cpu: [f32; SAMPLES],
    memory: [f32; SAMPLES],
}

impl Trace {
    pub(super) fn push(&mut self, gauges: Gauges) {
        for (readings, latest) in [
            (&mut self.cpu, gauges.cpu),
            (&mut self.memory, gauges.memory),
        ] {
            readings.rotate_left(1);
            readings[SAMPLES - 1] = latest;
        }
    }

    pub(super) fn latest(&self) -> Gauges {
        Gauges {
            cpu: self.cpu[SAMPLES - 1],
            memory: self.memory[SAMPLES - 1],
        }
    }

    /// The icon at each size it is offered at.
    pub(super) fn pixmaps(&self) -> Vec<Pixmap> {
        let raster = self.raster();
        MAGNIFICATIONS
            .into_iter()
            .map(|factor| magnified(&raster, factor))
            .collect()
    }

    /// A panel for each gauge, its letter on the left and its readings rising from the bottom, each
    /// turning from green to red as it climbs.
    fn raster(&self) -> Vec<u8> {
        let mut data = vec![0; SIZE * SIZE * 4];
        let panels = [
            (&CPU_LABEL, &self.cpu, self.colors.cpu),
            (&MEMORY_LABEL, &self.memory, self.colors.memory),
        ];
        for (index, (label, readings, colors)) in panels.into_iter().enumerate() {
            let top = index * (PANEL_HEIGHT + GAP);
            for row in top..top + PANEL_HEIGHT {
                for column in 0..SIZE {
                    paint(&mut data, row, column, colors.background);
                }
            }
            let label_top = top + (PANEL_HEIGHT - label.len()) / 2;
            for (y, line) in label.iter().enumerate() {
                for (x, mark) in line.bytes().enumerate() {
                    if mark == b'#' {
                        paint(&mut data, label_top + y, LABEL_X + x, colors.text);
                    }
                }
            }
            let bottom = top + PLOT_TOP + usize::from(PLOT_ROWS) - 1;
            for (offset, &level) in readings.iter().enumerate() {
                for lit in 0..rows(level) {
                    paint(
                        &mut data,
                        bottom - lit,
                        PLOT_X + offset,
                        colour(level, colors),
                    );
                }
            }
        }
        data
    }
}

/// The raster with each of its pixels drawn as a square `factor` pixels across.
fn magnified(raster: &[u8], factor: usize) -> Pixmap {
    let side = SIZE * factor;
    let mut data = Vec::with_capacity(side * side * 4);
    for row in 0..side {
        for column in 0..side {
            let at = (row / factor * SIZE + column / factor) * 4;
            data.extend_from_slice(&raster[at..at + 4]);
        }
    }
    let side = i32::try_from(side).unwrap_or(i32::MAX);
    (side, side, data)
}

fn paint(data: &mut [u8], row: usize, column: usize, [red, green, blue]: [u8; 3]) {
    let at = (row * SIZE + column) * 4;
    data[at..at + 4].copy_from_slice(&[0xff, red, green, blue]);
}

/// How many of the plot's rows a level fills.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=100, so the product lies in 0..=PLOT_ROWS"
)]
fn rows(level: f32) -> usize {
    (level.clamp(0.0, 100.0) / 100.0 * f32::from(PLOT_ROWS)).round() as usize
}

fn colour(level: f32, colors: GraphColors) -> [u8; 3] {
    if level < 60.0 {
        colors.low
    } else if level < 85.0 {
        colors.medium
    } else {
        colors.high
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(raster: &[u8], row: usize, column: usize) -> [u8; 4] {
        let at = (row * SIZE + column) * 4;
        raster[at..at + 4].try_into().unwrap()
    }

    fn opaque([red, green, blue]: [u8; 3]) -> [u8; 4] {
        [0xff, red, green, blue]
    }

    const NEWEST: usize = PLOT_X + SAMPLES - 1;
    const CPU_TOP: usize = 0;
    const MEMORY_TOP: usize = PANEL_HEIGHT + GAP;
    const PLOT_BOTTOM: usize = PLOT_TOP + PLOT_ROWS as usize - 1;

    fn traced(readings: &[(f32, f32)]) -> Trace {
        let mut trace = Trace::default();
        for &(cpu, memory) in readings {
            trace.push(Gauges { cpu, memory });
        }
        trace
    }

    #[test]
    fn the_icon_is_offered_as_squares_of_four_byte_pixels_at_whole_multiples_of_its_size() {
        let sizes: Vec<(i32, i32, usize)> = Trace::default()
            .pixmaps()
            .into_iter()
            .map(|(width, height, data)| (width, height, data.len()))
            .collect();
        assert_eq!(sizes, [(22, 22, 22 * 22 * 4), (44, 44, 44 * 44 * 4)]);
    }

    #[test]
    fn a_magnified_icon_draws_each_pixel_as_a_block_of_the_same_colour() {
        let trace = traced(&[(100.0, 0.0)]);
        let raster = trace.raster();
        let (_, _, data) = &trace.pixmaps()[1];
        let (row, column) = (CPU_TOP + PLOT_TOP, NEWEST);
        for (down, across) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            let at = ((row * 2 + down) * SIZE * 2 + column * 2 + across) * 4;
            assert_eq!(data[at..at + 4], pixel(&raster, row, column));
        }
        assert_eq!(pixel(&raster, row, column), opaque([0xe5, 0x39, 0x35]));
    }

    #[test]
    fn a_full_reading_is_lit_to_the_top_in_red() {
        let pixmap = traced(&[(100.0, 0.0)]).raster();
        assert_eq!(
            pixel(&pixmap, CPU_TOP + PLOT_TOP, NEWEST),
            opaque([0xe5, 0x39, 0x35])
        );
        assert_eq!(
            pixel(&pixmap, MEMORY_TOP + PLOT_TOP, NEWEST),
            opaque(BACKGROUND)
        );
    }

    #[test]
    fn a_half_reading_is_lit_from_the_bottom_in_green() {
        let pixmap = traced(&[(50.0, 50.0)]).raster();
        for top in [CPU_TOP, MEMORY_TOP] {
            assert_eq!(
                pixel(&pixmap, top + PLOT_BOTTOM, NEWEST),
                opaque([0x4c, 0xaf, 0x50])
            );
            assert_eq!(pixel(&pixmap, top + PLOT_TOP, NEWEST), opaque(BACKGROUND));
        }
    }

    #[test]
    fn an_earlier_reading_moves_left_as_the_next_one_arrives() {
        let pixmap = traced(&[(100.0, 0.0), (0.0, 0.0)]).raster();
        assert_eq!(
            pixel(&pixmap, CPU_TOP + PLOT_TOP, NEWEST - 1),
            opaque([0xe5, 0x39, 0x35])
        );
        assert_eq!(
            pixel(&pixmap, CPU_TOP + PLOT_TOP, NEWEST),
            opaque(BACKGROUND)
        );
    }

    #[test]
    fn a_reading_leaves_the_trace_once_it_is_older_than_the_window() {
        let mut readings = vec![(100.0, 100.0)];
        readings.resize(SAMPLES + 1, (0.0, 0.0));
        let pixmap = traced(&readings).raster();
        for column in PLOT_X..=NEWEST {
            assert_eq!(
                pixel(&pixmap, CPU_TOP + PLOT_BOTTOM, column),
                opaque(BACKGROUND)
            );
        }
    }

    #[test]
    fn nothing_is_traced_before_the_first_reading() {
        let pixmap = Trace::default().raster();
        for top in [CPU_TOP, MEMORY_TOP] {
            for column in PLOT_X..=NEWEST {
                assert_eq!(
                    pixel(&pixmap, top + PLOT_BOTTOM, column),
                    opaque(BACKGROUND)
                );
            }
        }
    }

    #[test]
    fn each_panel_carries_its_own_letter() {
        let pixmap = Trace::default().raster();
        let label_top = (PANEL_HEIGHT - CPU_LABEL.len()) / 2;
        let at = |top: usize, column: usize| pixel(&pixmap, top + label_top, column);
        assert_eq!(at(CPU_TOP, LABEL_X + 1), opaque(INK));
        assert_eq!(at(CPU_TOP, LABEL_X), opaque(BACKGROUND));
        assert_eq!(at(MEMORY_TOP, LABEL_X), opaque(INK));
        assert_eq!(at(MEMORY_TOP, LABEL_X + 1), opaque(BACKGROUND));
    }

    #[test]
    fn levels_beyond_the_scale_are_clamped() {
        assert_eq!(rows(250.0), usize::from(PLOT_ROWS));
        assert_eq!(rows(-5.0), 0);
    }

    #[test]
    fn the_tooltip_reads_the_latest_gauges() {
        let mut trace = Trace::default();
        trace.push(Gauges {
            cpu: 90.0,
            memory: 10.0,
        });
        trace.push(Gauges {
            cpu: 12.4,
            memory: 41.6,
        });
        assert_eq!(trace.latest().to_string(), "CPU 12%   Memory 42%");
    }
    #[test]
    fn recoloring_updates_existing_history_and_each_panel_independently() {
        let mut trace = traced(&[(50.0, 100.0)]);
        let old = trace.pixmaps();
        trace.colors.cpu.low = [1, 2, 3];
        trace.colors.memory.high = [4, 5, 6];
        trace.colors.memory.background = [7, 8, 9];
        trace.colors.cpu.text = [10, 11, 12];
        let raster = trace.raster();
        assert_ne!(old, trace.pixmaps());
        assert_eq!(pixel(&raster, PLOT_BOTTOM, NEWEST), opaque([1, 2, 3]));
        assert_eq!(
            pixel(&raster, MEMORY_TOP + PLOT_BOTTOM, NEWEST),
            opaque([4, 5, 6])
        );
        assert_eq!(pixel(&raster, MEMORY_TOP, 0), opaque([7, 8, 9]));
        assert_eq!(
            pixel(&raster, (PANEL_HEIGHT - CPU_LABEL.len()) / 2, LABEL_X + 1),
            opaque([10, 11, 12])
        );
    }
}
