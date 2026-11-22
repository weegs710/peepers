use std::sync::LazyLock;

use iced::Theme;
use iced::theme::Palette;
use serde::Deserialize;

/// Palettes in noctalia's custom palette format, shipped as they were written.
const SHIPPED: [(&str, &str); 6] = [
    ("Cold Wind", include_str!("../themes/Cold-Wind.json")),
    ("Cymagentan", include_str!("../themes/Cymagentan.json")),
    ("Great Below", include_str!("../themes/Great-Below.json")),
    ("Plasm", include_str!("../themes/Plasm.json")),
    ("Triglavian", include_str!("../themes/Triglavian.json")),
    ("Twilight", include_str!("../themes/Twilight.json")),
];

/// Every theme that can be chosen: iced's own, then the shipped palettes.
static ALL: LazyLock<Vec<Theme>> = LazyLock::new(|| {
    Theme::ALL
        .iter()
        .cloned()
        .chain(SHIPPED.iter().map(|&(name, json)| {
            let file: File = serde_json::from_str(json).expect("a shipped palette is valid");
            Theme::custom(name, file.dark.palette())
        }))
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
        )
    }
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

/// Success and warning keep iced's own colours, so they read the same whatever the palette.
fn palette(
    background: iced::Color,
    text: iced::Color,
    primary: iced::Color,
    danger: iced::Color,
) -> Palette {
    Palette {
        background,
        text,
        primary,
        danger,
        ..Palette::DARK
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
        let shipped = &all()[Theme::ALL.len()..];
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
