use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::Arc;

use iced::Color;

pub mod styles;
use iced::theme::Palette;
use serde::Deserialize;

/// Embedded so standalone binaries can create a complete, editable reference.
pub const TEMPLATE: &str = include_str!("../../../packaging/themes/default.toml");

pub fn write_template(directory: &Path) -> Result<PathBuf, String> {
    let path = directory.join("default.toml");
    fs::create_dir_all(directory)
        .and_then(|()| crate::storage::write_new(&path, TEMPLATE))
        .map_err(|error| {
            if error.kind() == ErrorKind::AlreadyExists {
                format!(
                    "{} already exists; your file was not overwritten.",
                    path.display()
                )
            } else {
                format!("Could not create {}: {error}", path.display())
            }
        })?;
    Ok(path)
}

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
    iced::Theme::ALL
        .iter()
        .cloned()
        .map(Theme::from)
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
    /// Identity and change times also catch atomic replacement and permission changes.
    seen: Option<FileStamp>,
    theme: Option<Theme>,
    problem: Option<String>,
}

#[derive(Debug, PartialEq)]
struct FileStamp {
    device: u64,
    inode: u64,
    modified: (i64, i64),
    changed: (i64, i64),
    len: u64,
}
impl From<fs::Metadata> for FileStamp {
    fn from(meta: fs::Metadata) -> Self {
        Self {
            device: meta.dev(),
            inode: meta.ino(),
            modified: (meta.mtime(), meta.mtime_nsec()),
            changed: (meta.ctime(), meta.ctime_nsec()),
            len: meta.len(),
        }
    }
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
        self.refresh_named(NOCTALIA);
    }

    fn refresh_named(&mut self, name: &str) {
        let Some(path) = &self.path else { return };
        let stamp = fs::metadata(path).ok().map(FileStamp::from);
        let read_before = self.theme.is_some() || self.problem.is_some();
        if read_before && stamp == self.seen {
            return;
        }
        self.seen = stamp;
        match read_named(path, name) {
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

/// Both custom themes and Noctalia use this format. Old four-colour files remain valid.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Written {
    mode: Mode,
    base: Option<String>,
    #[serde(alias = "background")]
    surface: Option<Hex>,
    #[serde(alias = "text")]
    on_surface: Option<Hex>,
    primary: Option<Hex>,
    #[serde(alias = "danger")]
    error: Option<Hex>,
    success: Option<Hex>,
    warning: Option<Hex>,
    ui: BTreeMap<String, Hex>,
    widgets: BTreeMap<String, BTreeMap<String, Hex>>,
    extended: BTreeMap<String, BTreeMap<String, Pair>>,
    tray: TrayStyle,
    graph: GraphStyle,
    graphs: BTreeMap<String, GraphRule>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    #[default]
    Dark,
    Light,
}

fn read_named(path: &Path, name: &str) -> Result<Theme, String> {
    let shown = path.display();
    let contents = fs::read_to_string(path).map_err(|error| match error.kind() {
        ErrorKind::NotFound => format!("{shown} has not been written yet, so Dark is in use."),
        _ => format!("{shown} could not be read, so Dark is in use: {error}"),
    })?;
    parse(&contents, name)
        .map_err(|error| format!("{shown} could not be read, so Dark is in use: {error}"))
}

pub(super) fn parse(contents: &str, name: &str) -> Result<Theme, String> {
    let written: Written = toml_edit::de::from_str(contents).map_err(|error| error.to_string())?;
    let mut palette = if let Some(base) = &written.base {
        all()
            .iter()
            .find(|theme| theme.to_string() == *base && base != NOCTALIA)
            .ok_or_else(|| format!("unknown base theme {base}"))?
            .palette()
    } else {
        match written.mode {
            Mode::Dark => Palette::DARK,
            Mode::Light => Palette::LIGHT,
        }
    };
    for (target, value) in [
        (&mut palette.background, written.surface),
        (&mut palette.text, written.on_surface),
        (&mut palette.primary, written.primary),
        (&mut palette.danger, written.error),
        (&mut palette.success, written.success),
        (&mut palette.warning, written.warning),
    ] {
        if let Some(value) = value {
            *target = value.0;
        }
    }
    let mut extended = iced::theme::palette::Extended::generate(palette);
    for (family, pairs) in &written.extended {
        for (shade, pair) in pairs {
            let target = extended_pair(&mut extended, family, shade)
                .ok_or_else(|| format!("unknown extended palette pair {family}.{shade}"))?;
            if let Some(color) = pair.color {
                target.color = color.0;
            }
            if let Some(text) = pair.text {
                target.text = text.0;
            }
        }
    }
    let widgets = styles::compile(written.widgets)?;
    written.tray.validate()?;
    validate_graphs(&written.graph, &written.graphs)?;
    for key in written.ui.keys() {
        if !UI_ROLES.contains(&key.as_str()) {
            return Err(format!("unknown ui colour {key}"));
        }
    }
    Ok(Theme {
        inner: iced::Theme::custom_with_fn(name.to_owned(), palette, |_| extended),
        overrides: Some(Arc::new(Overrides {
            ui: written.ui,
            widgets,
            tray: written.tray,
            graph: written.graph,
            graphs: written.graphs,
        })),
    })
}

/// A colour written as `#rrggbb`.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(try_from = "String")]
pub struct Hex(pub Color);

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
        let shipped = &all()[iced::Theme::ALL.len()..all().len() - 1];
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

/// Application colours travel with the theme, including through canvas and widget rendering.
/// No global active palette: previews and multiple windows can render independently.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    inner: iced::Theme,
    overrides: Option<Arc<Overrides>>,
}

pub type Element<'a, Message> = iced::Element<'a, Message, Theme>;

impl From<iced::Theme> for Theme {
    fn from(inner: iced::Theme) -> Self {
        Self {
            inner,
            overrides: None,
        }
    }
}

impl std::ops::Deref for Theme {
    type Target = iced::Theme;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl std::fmt::Display for Theme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::Dark
    }
}

#[allow(non_upper_case_globals)]
impl Theme {
    pub const Dark: Self = Self {
        inner: iced::Theme::Dark,
        overrides: None,
    };
    #[cfg(test)]
    pub const Light: Self = Self {
        inner: iced::Theme::Light,
        overrides: None,
    };

    pub fn custom(name: impl Into<Cow<'static, str>>, palette: Palette) -> Self {
        iced::Theme::custom(name, palette).into()
    }

    pub fn color(&self, role: &str, fallback: Color) -> Color {
        self.override_color(role).unwrap_or(fallback)
    }

    pub fn override_color(&self, role: &str) -> Option<Color> {
        self.overrides
            .as_ref()
            .and_then(|o| o.ui.get(role))
            .map(|hex| hex.0)
    }

    pub fn tray_colors(&self) -> peepers_core::tray::IconColors {
        let palette = self.palette();
        let make = |panel: &str| {
            let rgb = |key: &str, fallback: Color| {
                let color = self
                    .overrides
                    .as_ref()
                    .and_then(|o| o.tray.color(panel, key))
                    .unwrap_or(fallback);
                let [red, green, blue, _] = color.into_rgba8();
                [red, green, blue]
            };
            peepers_core::tray::GraphColors {
                background: rgb("background", palette.background),
                text: rgb("text", palette.text),
                low: rgb("low", palette.success),
                medium: rgb("medium", palette.warning),
                high: rgb("high", palette.danger),
            }
        };
        peepers_core::tray::IconColors {
            cpu: make("cpu"),
            memory: make("memory"),
        }
    }

    pub fn graph_color(
        &self,
        graph: &str,
        instance: Option<&str>,
        line: Option<&str>,
        part: &str,
    ) -> Option<Color> {
        self.graph_colors(graph, instance, line, &[part])
    }

    pub fn graph_colors(
        &self,
        graph: &str,
        instance: Option<&str>,
        line: Option<&str>,
        parts: &[&str],
    ) -> Option<Color> {
        let overrides = self.overrides.as_ref()?;
        let rule = overrides.graphs.get(graph);
        let specific = instance.and_then(|id| rule?.instances.get(id));
        let general = rule.map(|rule| &rule.style);
        [specific, general, Some(&overrides.graph)]
            .into_iter()
            .flatten()
            .find_map(|style| {
                line.and_then(|line| style.lines.get(line))
                    .and_then(|line| parts.iter().find_map(|part| line.get(*part)))
                    .or_else(|| parts.iter().find_map(|part| style.colors.get(*part)))
                    .map(|color| color.0)
            })
    }
}

impl iced::theme::Base for Theme {
    fn default(mode: iced::theme::Mode) -> Self {
        <iced::Theme as iced::theme::Base>::default(mode).into()
    }
    fn mode(&self) -> iced::theme::Mode {
        self.inner.mode()
    }
    fn base(&self) -> iced::theme::Style {
        self.inner.base()
    }
    fn palette(&self) -> Option<Palette> {
        Some(self.inner.palette())
    }
    fn name(&self) -> &str {
        self.inner.name()
    }
}

const UI_ROLES: &[&str] = &[
    "success_text",
    "danger_text",
    "heading",
    "section_heading",
    "table_heading",
    "label",
    "value",
    "secondary_text",
    "icon",
    "selected_icon",
    "row_hover",
    "row_selected",
    "separator",
    "border",
    "panel",
    "panel_text",
    "tooltip",
    "tooltip_text",
];

#[derive(Debug, Default, PartialEq)]
struct Overrides {
    ui: BTreeMap<String, Hex>,
    widgets: styles::Widgets,
    tray: TrayStyle,
    graph: GraphStyle,
    graphs: BTreeMap<String, GraphRule>,
}

#[derive(Debug, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Pair {
    color: Option<Hex>,
    text: Option<Hex>,
}

// Flat colour keys keep graph defaults and individual graph/line overrides identical.
#[derive(Debug, Default, PartialEq, Deserialize)]
struct GraphStyle {
    #[serde(default)]
    lines: BTreeMap<String, BTreeMap<String, Hex>>,
    #[serde(flatten)]
    colors: BTreeMap<String, Hex>,
}

#[derive(Debug, Default, PartialEq, Deserialize)]
struct GraphRule {
    #[serde(default)]
    instances: BTreeMap<String, GraphStyle>,
    #[serde(flatten)]
    style: GraphStyle,
}

#[derive(Debug, Default, PartialEq, Deserialize)]
struct TrayStyle {
    #[serde(default)]
    cpu: BTreeMap<String, Hex>,
    #[serde(default)]
    memory: BTreeMap<String, Hex>,
    #[serde(flatten)]
    colors: BTreeMap<String, Hex>,
}

impl TrayStyle {
    fn validate(&self) -> Result<(), String> {
        for key in self
            .colors
            .keys()
            .chain(self.cpu.keys())
            .chain(self.memory.keys())
        {
            if !["background", "text", "low", "medium", "high"].contains(&key.as_str()) {
                return Err(format!("unknown tray colour {key}"));
            }
        }
        Ok(())
    }
    fn color(&self, panel: &str, key: &str) -> Option<Color> {
        let colors = if panel == "cpu" {
            &self.cpu
        } else {
            &self.memory
        };
        colors
            .get(key)
            .or_else(|| self.colors.get(key))
            .map(|hex| hex.0)
    }
}

fn extended_pair<'a>(
    extended: &'a mut iced::theme::palette::Extended,
    family: &str,
    shade: &str,
) -> Option<&'a mut iced::theme::palette::Pair> {
    macro_rules! shades {
        ($group:expr) => {
            match shade {
                "base" => Some(&mut $group.base),
                "weak" => Some(&mut $group.weak),
                "strong" => Some(&mut $group.strong),
                _ => None,
            }
        };
    }
    match family {
        "background" => match shade {
            "neutral" => Some(&mut extended.background.neutral),
            "weakest" => Some(&mut extended.background.weakest),
            "weaker" => Some(&mut extended.background.weaker),
            "stronger" => Some(&mut extended.background.stronger),
            "strongest" => Some(&mut extended.background.strongest),
            _ => shades!(extended.background),
        },
        "primary" => shades!(extended.primary),
        "secondary" => shades!(extended.secondary),
        "success" => shades!(extended.success),
        "warning" => shades!(extended.warning),
        "danger" => shades!(extended.danger),
        _ => None,
    }
}

/// A directory of selectable files. The filename is the persistent identity, avoiding duplicate names.
#[derive(Debug, Default)]
pub struct CustomThemes {
    directory: Option<PathBuf>,
    files: BTreeMap<String, Noctalia>,
    choices: Vec<Theme>,
    choice_names: Vec<String>,
    problem: Option<String>,
}

impl CustomThemes {
    pub fn directory(&self) -> Option<&Path> {
        self.directory.as_deref()
    }

    pub fn is_configured(&self) -> bool {
        self.directory.is_some()
    }

    pub fn at(directory: PathBuf) -> Self {
        Self {
            directory: Some(directory),
            ..Self::default()
        }
    }

    pub fn refresh(&mut self, selected: &str) {
        self.refresh_files(selected);
        if let Some(file) = self.files.get_mut(selected) {
            file.refresh_named(selected);
        }
        let mut names: Vec<_> = self.files.keys().map(String::as_str).collect();
        if selected.starts_with("Custom: ") && !self.files.contains_key(selected) {
            names.push(selected);
        }
        if self.choices.is_empty()
            || !self
                .choice_names
                .iter()
                .map(String::as_str)
                .eq(names.iter().copied())
        {
            self.choice_names = names.iter().map(|name| (*name).to_owned()).collect();
            self.choices = all().to_vec();
            self.choices.extend(
                names
                    .into_iter()
                    .map(|name| Theme::custom(name.to_owned(), Palette::DARK)),
            );
        }
    }

    fn refresh_files(&mut self, selected: &str) {
        let Some(directory) = &self.directory else {
            return;
        };
        let mut paths = BTreeMap::new();
        match fs::read_dir(directory) {
            Ok(entries) => {
                self.problem = None;
                for entry in entries {
                    match entry {
                        Ok(entry) => {
                            let path = entry.path();
                            if path.extension().is_some_and(|ext| ext == "toml")
                                && path.is_file()
                                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                            {
                                paths.insert(format!("Custom: {stem}"), path);
                            }
                        }
                        Err(error) => {
                            self.problem = Some(format!("Could not list themes: {error}"));
                        }
                    }
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => self.problem = None,
            Err(error) => {
                self.problem = Some(format!("Could not list {}: {error}", directory.display()));
                return;
            }
        }
        self.files
            .retain(|name, _| paths.contains_key(name) || name == selected);
        for (name, path) in paths {
            self.files.entry(name).or_insert_with(|| Noctalia::at(path));
        }
    }

    pub fn choices(&self) -> &[Theme] {
        if self.choices.is_empty() {
            all()
        } else {
            &self.choices
        }
    }

    pub fn theme(&self, name: &str) -> Option<&Theme> {
        self.files.get(name)?.theme()
    }

    pub fn problem(&self, name: &str) -> Option<&str> {
        self.problem.as_deref().or_else(|| {
            if name.starts_with("Custom: ") {
                self.files.get(name).map_or(
                    Some("The selected theme file is missing, so Dark is in use."),
                    Noctalia::problem,
                )
            } else {
                None
            }
        })
    }
}

fn graph_lines(key: &str) -> Option<&'static [&'static str]> {
    Some(match key {
        "cpu" | "cpu.core" | "flight.cpu" => &["utilization"],
        "memory" | "flight.memory" => &["used"],
        "disk.activity" => &["active"],
        "disk.io" | "flight.disk" => &["read", "write"],
        "network" | "flight.network" => &["receive", "send"],
        "gpu" | "flight.gpu" => &["utilization", "memory"],
        "clock" => &["frequency"],
        "sensor" | "energy" | "energy.activity" | "flight.sensor" => &["value"],
        "pressure" => &["stalled"],
        _ => return None,
    })
}
const LINE_COLORS: &[&str] = &[
    "color", "stroke", "fill", "glow", "point", "primary", "success", "warning",
];
fn validate_graphs(
    default: &GraphStyle,
    graphs: &BTreeMap<String, GraphRule>,
) -> Result<(), String> {
    fn style(style: &GraphStyle) -> Result<(), String> {
        for key in style.colors.keys() {
            if !LINE_COLORS.contains(&key.as_str())
                && ![
                    "background",
                    "grid",
                    "axis",
                    "legend",
                    "tooltip",
                    "marker",
                    "crosshair",
                ]
                .contains(&key.as_str())
            {
                return Err(format!("unknown graph colour {key}"));
            }
        }
        for (line, colors) in &style.lines {
            for key in colors.keys() {
                if !LINE_COLORS.contains(&key.as_str()) {
                    return Err(format!("unknown graph line colour {line}.{key}"));
                }
            }
        }
        Ok(())
    }
    style(default)?;
    for (key, graph) in graphs {
        let lines = graph_lines(key).ok_or_else(|| format!("unknown graph {key}"))?;
        for scoped in std::iter::once(&graph.style).chain(graph.instances.values()) {
            style(scoped)?;
            for line in scoped.lines.keys() {
                if !lines.contains(&line.as_str()) {
                    return Err(format!("unknown line {key}.{line}"));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod custom_tests;
