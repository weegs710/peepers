use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::SystemTime;

use iced::Theme;
use iced::theme::Palette;
use serde::Deserialize;

/// The theme whose colours come from the file noctalia writes for peepers.
pub const NOCTALIA: &str = "Noctalia";

/// Palettes in noctalia's custom palette format, shipped as they were written.
const SHIPPED: [(&str, &str); 6] = [
    ("Cold Wind", include_str!("../themes/Cold-Wind.json")),
    ("Cymagentan", include_str!("../themes/Cymagentan.json")),
    ("Great Below", include_str!("../themes/Great-Below.json")),
    ("Plasm", include_str!("../themes/Plasm.json")),
    ("Triglavian", include_str!("../themes/Triglavian.json")),
    ("Twilight", include_str!("../themes/Twilight.json")),
];

/// Every theme that can be chosen: iced's own, the shipped palettes, and noctalia's, which is listed
/// with stand-in colours because its own are only known once its file is read.
static ALL: LazyLock<Vec<Theme>> = LazyLock::new(|| {
    Theme::ALL
        .iter()
        .cloned()
        .chain(SHIPPED.iter().map(|&(name, json)| {
            let file: File = serde_json::from_str(json).expect("a shipped palette is valid");
            Theme::custom(name, file.dark.palette())
        }))
        .chain([Theme::custom(NOCTALIA, Palette::DARK)])
        .collect()
});

pub fn all() -> &'static [Theme] {
    &ALL
}

/// The part of a noctalia palette file peepers draws with. Only the dark set is read.
#[derive(Deserialize)]
struct File {
    dark: Roles,
}

#[derive(Deserialize)]
struct Roles {
    #[serde(rename = "mSurface")]
    surface: Hex,
    #[serde(rename = "mOnSurface")]
    on_surface: Hex,
    #[serde(rename = "mPrimary")]
    primary: Hex,
    #[serde(rename = "mError")]
    error: Hex,
}

impl Roles {
    fn palette(&self) -> Palette {
        palette(
            self.surface.0,
            self.on_surface.0,
            self.primary.0,
            self.error.0,
            Palette::DARK,
        )
    }
}

/// The colours noctalia's template writes for peepers, and what was last seen of their file.
#[derive(Debug, Default)]
pub struct Noctalia {
    /// Where the file is looked for; without one nothing is read.
    path: Option<PathBuf>,
    /// The file's modification time and length when it was last read.
    seen: Option<(SystemTime, u64)>,
    theme: Option<Theme>,
    problem: Option<String>,
}

impl Noctalia {
    pub fn at(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            ..Self::default()
        }
    }

    pub fn theme(&self) -> Option<&Theme> {
        self.theme.as_ref()
    }

    /// Why noctalia's colours are not in use, when they are not.
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    /// Reads the file again when it has changed since it was last read, which is how a new
    /// noctalia palette reaches peepers without a restart.
    pub fn refresh(&mut self) {
        let Some(path) = &self.path else { return };
        let stamp = fs::metadata(path)
            .ok()
            .and_then(|meta| Some((meta.modified().ok()?, meta.len())));
        let read_before = self.theme.is_some() || self.problem.is_some();
        if read_before && stamp == self.seen {
            return;
        }
        self.seen = stamp;
        match read(path) {
            Ok(theme) => {
                self.theme = Some(theme);
                self.problem = None;
            }
            Err(problem) => {
                self.theme = None;
                self.problem = Some(problem);
            }
        }
    }
}

/// The file noctalia renders from `packaging/noctalia/colors.toml`.
#[derive(Deserialize)]
struct Written {
    mode: Mode,
    surface: Hex,
    on_surface: Hex,
    primary: Hex,
    error: Hex,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    Dark,
    Light,
}

fn read(path: &Path) -> Result<Theme, String> {
    let shown = path.display();
    let contents = fs::read_to_string(path).map_err(|error| match error.kind() {
        ErrorKind::NotFound => format!(
            "noctalia has not written {shown} yet, so Dark is in use. Add the peepers template to noctalia."
        ),
        _ => format!("{shown} could not be read, so Dark is in use: {error}"),
    })?;
    let written: Written = toml_edit::de::from_str(&contents)
        .map_err(|error| format!("{shown} could not be read, so Dark is in use: {error}"))?;
    let fixed = match written.mode {
        Mode::Dark => Palette::DARK,
        Mode::Light => Palette::LIGHT,
    };
    Ok(Theme::custom(
        NOCTALIA,
        palette(
            written.surface.0,
            written.on_surface.0,
            written.primary.0,
            written.error.0,
            fixed,
        ),
    ))
}

/// A colour written as `#rrggbb`.
#[derive(Deserialize)]
#[serde(try_from = "String")]
struct Hex(iced::Color);

impl TryFrom<String> for Hex {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        text.parse()
            .map(Self)
            .map_err(|_| format!("{text} is not a colour"))
    }
}

/// Success and warning keep iced's own colours from `fixed`, so they read the same whatever the
/// palette.
fn palette(
    background: iced::Color,
    text: iced::Color,
    primary: iced::Color,
    danger: iced::Color,
    fixed: Palette,
) -> Palette {
    Palette {
        background,
        text,
        primary,
        danger,
        ..fixed
    }
}

#[cfg(test)]
mod tests {
    use iced::Color;

    use super::*;

    fn rgb(hex: &str) -> Color {
        hex.parse().unwrap()
    }

    #[test]
    fn every_shipped_palette_is_listed_after_iceds_own_with_its_own_colours() {
        let shipped = &all()[Theme::ALL.len()..all().len() - 1];
        let expected = [
            ("Cold Wind", "#05070b", "#eaf3fa", "#33c9ff", "#e07a9c"),
            ("Cymagentan", "#2a122c", "#f3f2f3", "#de67e4", "#fd4663"),
            ("Great Below", "#060505", "#efe6d6", "#ff2f45", "#ff6a3d"),
            ("Plasm", "#04060a", "#e6f2ec", "#3dffb0", "#ff5d8a"),
            ("Triglavian", "#060404", "#e8ecf1", "#ff2a22", "#ff6b7a"),
            ("Twilight", "#060811", "#e7e9f6", "#2ee6d6", "#ff5470"),
        ];
        assert_eq!(shipped.len(), expected.len());
        for (theme, (name, background, text, primary, danger)) in shipped.iter().zip(expected) {
            assert_eq!(theme.to_string(), name);
            let palette = theme.palette();
            assert_eq!(palette.background, rgb(background), "{name}");
            assert_eq!(palette.text, rgb(text), "{name}");
            assert_eq!(palette.primary, rgb(primary), "{name}");
            assert_eq!(palette.danger, rgb(danger), "{name}");
            assert_eq!(palette.success, Palette::DARK.success, "{name}");
            assert_eq!(palette.warning, Palette::DARK.warning, "{name}");
        }
    }

    #[test]
    fn no_two_themes_share_a_name() {
        let mut names: Vec<String> = all().iter().map(ToString::to_string).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), all().len());
    }
}
