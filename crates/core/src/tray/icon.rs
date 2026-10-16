use std::fmt;

use crate::telemetry::Vitals;

/// Icons travel as width, height and ARGB32 bytes in network order.
pub(super) type Pixmap = (i32, i32, Vec<u8>);

const SIZE: usize = 32;
const MARGIN: usize = 2;
const BAR_WIDTH: usize = 12;
const BAR_HEIGHT: u8 = 28;
const TRACK: [u8; 3] = [0x60, 0x60, 0x60];

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

impl Gauges {
    /// One bar per gauge, filling from the bottom and turning from green to red as it rises.
    pub(super) fn pixmap(self) -> Pixmap {
        let bars = [(MARGIN, self.cpu), (SIZE - MARGIN - BAR_WIDTH, self.memory)];
        let mut data = vec![0; SIZE * SIZE * 4];
        for (left, level) in bars {
            let filled = rows(level);
            for row in 0..usize::from(BAR_HEIGHT) {
                let lit = usize::from(BAR_HEIGHT) - row <= filled;
                let [red, green, blue] = if lit { colour(level) } else { TRACK };
                for column in left..left + BAR_WIDTH {
                    let at = ((row + MARGIN) * SIZE + column) * 4;
                    data[at..at + 4].copy_from_slice(&[0xff, red, green, blue]);
                }
            }
        }
        let side = i32::try_from(SIZE).unwrap_or(i32::MAX);
        (side, side, data)
    }
}

/// How many of the bar's rows a level fills.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=100, so the product lies in 0..=BAR_HEIGHT"
)]
fn rows(level: f32) -> usize {
    (level.clamp(0.0, 100.0) / 100.0 * f32::from(BAR_HEIGHT)).round() as usize
}

fn colour(level: f32) -> [u8; 3] {
    if level < 60.0 {
        [0x4c, 0xaf, 0x50]
    } else if level < 85.0 {
        [0xff, 0xb3, 0x00]
    } else {
        [0xe5, 0x39, 0x35]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(pixmap: &Pixmap, row: usize, column: usize) -> &[u8] {
        let at = (row * SIZE + column) * 4;
        &pixmap.2[at..at + 4]
    }

    const BOTTOM: usize = MARGIN + BAR_HEIGHT as usize - 1;
    const TOP: usize = MARGIN;
    const MEMORY: usize = SIZE - MARGIN - BAR_WIDTH;

    #[test]
    fn the_pixmap_is_a_square_of_four_byte_pixels() {
        let (width, height, data) = Gauges::default().pixmap();
        assert_eq!((width, height), (32, 32));
        assert_eq!(data.len(), 32 * 32 * 4);
    }

    #[test]
    fn a_full_bar_is_lit_to_the_top_in_red() {
        let pixmap = Gauges {
            cpu: 100.0,
            memory: 0.0,
        }
        .pixmap();
        assert_eq!(pixel(&pixmap, TOP, MARGIN), [0xff, 0xe5, 0x39, 0x35]);
        assert_eq!(pixel(&pixmap, TOP, MEMORY), [0xff, 0x60, 0x60, 0x60]);
    }

    #[test]
    fn a_half_bar_is_lit_from_the_bottom_in_green() {
        let pixmap = Gauges {
            cpu: 50.0,
            memory: 50.0,
        }
        .pixmap();
        assert_eq!(pixel(&pixmap, BOTTOM, MARGIN), [0xff, 0x4c, 0xaf, 0x50]);
        assert_eq!(pixel(&pixmap, TOP, MARGIN), [0xff, 0x60, 0x60, 0x60]);
        assert_eq!(pixel(&pixmap, BOTTOM, MEMORY), [0xff, 0x4c, 0xaf, 0x50]);
    }

    #[test]
    fn an_empty_bar_shows_only_the_track() {
        let pixmap = Gauges::default().pixmap();
        assert_eq!(pixel(&pixmap, BOTTOM, MARGIN), [0xff, 0x60, 0x60, 0x60]);
    }

    #[test]
    fn levels_beyond_the_scale_are_clamped() {
        assert_eq!(rows(250.0), usize::from(BAR_HEIGHT));
        assert_eq!(rows(-5.0), 0);
    }

    #[test]
    fn the_tooltip_reads_the_gauges() {
        let gauges = Gauges {
            cpu: 12.4,
            memory: 41.6,
        };
        assert_eq!(gauges.to_string(), "CPU 12%   Memory 42%");
    }
}
