//! Keep iced's widget behaviour and derived colours; expose application roles separately.
use super::{Hex, Theme};
use iced::widget::overlay::menu;
use iced::widget::{
    button, checkbox, container, pick_list, progress_bar, rule, scrollable, slider, table, text,
    text_input,
};
use std::collections::BTreeMap;

macro_rules! catalog {
    ($module:ident, $default:ident) => {
        impl $module::Catalog for Theme {
            type Class<'a> = $module::StyleFn<'a, Self>;
            fn default<'a>() -> $module::StyleFn<'a, Self> {
                Box::new(|theme| {
                    let mut style = $module::$default(theme);
                    apply(theme, stringify!($module), None, "", &mut style);
                    style
                })
            }
            fn style(&self, class: &$module::StyleFn<'_, Self>) -> $module::Style {
                class(self)
            }
        }
    };
    ($module:ident, $default:ident, status) => {
        impl $module::Catalog for Theme {
            type Class<'a> = $module::StyleFn<'a, Self>;
            fn default<'a>() -> $module::StyleFn<'a, Self> {
                Box::new(|theme, status| {
                    let mut style = $module::$default(theme, status);
                    apply(theme, stringify!($module), None, status.key(), &mut style);
                    style
                })
            }
            fn style(
                &self,
                class: &$module::StyleFn<'_, Self>,
                status: $module::Status,
            ) -> $module::Style {
                class(self, status)
            }
        }
    };
}
catalog!(text, default);
catalog!(container, transparent);
impl button::Catalog for Theme {
    type Class<'a> = button::StyleFn<'a, Self>;
    fn default<'a>() -> Self::Class<'a> {
        Box::new(button_primary)
    }
    fn style(&self, class: &Self::Class<'_>, status: button::Status) -> button::Style {
        class(self, status)
    }
}
catalog!(checkbox, primary, status);
catalog!(pick_list, default, status);
catalog!(scrollable, default, status);
catalog!(slider, default, status);
catalog!(text_input, default, status);
catalog!(menu, default);
catalog!(progress_bar, primary);

impl table::Catalog for Theme {
    type Class<'a> = table::StyleFn<'a, Self>;
    fn default<'a>() -> Self::Class<'a> {
        Box::new(|theme| {
            let mut style = table::default(theme);
            if let Some(color) = theme.overrides.as_ref().and_then(|o| o.ui.get("separator")) {
                style.separator_x = color.0.into();
                style.separator_y = color.0.into();
            }
            apply(theme, "table", None, "", &mut style);
            style
        })
    }
    fn style(&self, class: &Self::Class<'_>) -> table::Style {
        class(self)
    }
}
impl rule::Catalog for Theme {
    type Class<'a> = rule::StyleFn<'a, Self>;
    fn default<'a>() -> Self::Class<'a> {
        Box::new(|theme| {
            let mut style = rule::default(theme);
            style.color = theme.color("separator", style.color);
            apply(theme, "rule", None, "", &mut style);
            style
        })
    }
    fn style(&self, class: &Self::Class<'_>) -> rule::Style {
        class(self)
    }
}

macro_rules! text_style {
    ($name:ident, $base:ident, $role:literal) => {
        pub fn $name(theme: &Theme) -> text::Style {
            let mut style = text::$base(theme);
            apply(theme, "text", None, "", &mut style);
            if let Some(color) = theme.overrides.as_ref().and_then(|o| o.ui.get($role)) {
                style.color = Some(color.0);
            }
            style
        }
    };
}
text_style!(text_secondary, secondary, "secondary_text");
text_style!(text_success, success, "success_text");
text_style!(text_danger, danger, "danger_text");
text_style!(heading, default, "heading");
pub fn label(theme: &Theme) -> text::Style {
    let mut style = text_secondary(theme);
    if let Some(color) = theme.overrides.as_ref().and_then(|o| o.ui.get("label")) {
        style.color = Some(color.0);
    }
    style
}
text_style!(value, default, "value");
pub fn section_heading(theme: &Theme) -> text::Style {
    let mut style = heading(theme);
    if let Some(color) = theme
        .overrides
        .as_ref()
        .and_then(|o| o.ui.get("section_heading"))
    {
        style.color = Some(color.0);
    }
    style
}
pub fn table_heading(theme: &Theme) -> text::Style {
    let mut style = heading(theme);
    if let Some(color) = theme
        .overrides
        .as_ref()
        .and_then(|o| o.ui.get("table_heading"))
    {
        style.color = Some(color.0);
    }
    style
}
macro_rules! button_style {
    ($name:ident, $base:ident) => {
        pub fn $name(theme: &Theme, status: button::Status) -> button::Style {
            let mut style = button::$base(theme, status);
            apply(
                theme,
                "button",
                Some(stringify!($base)),
                status.key(),
                &mut style,
            );
            style
        }
    };
}
button_style!(button_primary, primary);
button_style!(button_secondary, secondary);
button_style!(button_success, success);
button_style!(button_danger, danger);
button_style!(button_text, text);

pub fn container_rounded_box(theme: &Theme) -> container::Style {
    panel(theme, container::rounded_box(theme))
}
pub fn container_bordered_box(theme: &Theme) -> container::Style {
    panel(theme, container::bordered_box(theme))
}
fn panel(theme: &Theme, mut style: container::Style) -> container::Style {
    if let Some(overrides) = &theme.overrides {
        if let Some(color) = overrides.ui.get("panel") {
            style.background = Some(color.0.into());
        }
        if let Some(color) = overrides.ui.get("panel_text") {
            style.text_color = Some(color.0);
        }
    }
    style.border.color = theme.color("border", style.border.color);
    apply(theme, "container", None, "", &mut style);
    style
}

pub fn text_default(theme: &Theme) -> text::Style {
    let mut style = text::default(theme);
    apply(theme, "text", None, "", &mut style);
    style
}

trait State {
    fn key(self) -> &'static str;
}
macro_rules! states {
    ($module:ident, $($variant:pat => $name:literal),* $(,)?) => {
        impl State for $module::Status {
            fn key(self) -> &'static str { match self { $($variant => $name),* } }
        }
    };
}
states!(button, Self::Active => "active", Self::Hovered => "hovered", Self::Pressed => "pressed", Self::Disabled => "disabled");
states!(checkbox, Self::Active { .. } => "active", Self::Hovered { .. } => "hovered", Self::Disabled { .. } => "disabled");
states!(pick_list, Self::Active => "active", Self::Hovered => "hovered", Self::Opened { .. } => "opened");
states!(text_input, Self::Active => "active", Self::Hovered => "hovered", Self::Focused { .. } => "focused", Self::Disabled => "disabled");
states!(slider, Self::Active => "active", Self::Hovered => "hovered", Self::Dragged => "dragged");
states!(scrollable, Self::Active { .. } => "active", Self::Hovered { .. } => "hovered", Self::Dragged { .. } => "dragged");

type Colors = BTreeMap<String, Hex>;

/// Paths are compiled once, so widget rendering only borrows lookup keys.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Widgets(BTreeMap<String, WidgetRules>);
#[derive(Debug, Default, PartialEq)]
struct WidgetRules {
    colors: Colors,
    states: BTreeMap<String, Colors>,
    classes: BTreeMap<String, WidgetRules>,
}

pub(super) fn compile(widgets: BTreeMap<String, Colors>) -> Result<Widgets, String> {
    validate(&widgets)?;
    let mut compiled = Widgets::default();
    for (path, colors) in widgets {
        let mut parts = path.split('.');
        let widget = parts.next().expect("validated widget path");
        let mut rules = compiled.0.entry(widget.to_owned()).or_default();
        let mut state = parts.next();
        if widget == "button" && state.is_some_and(|part| BUTTON_CLASSES.contains(&part)) {
            rules = rules.classes.entry(state.unwrap().to_owned()).or_default();
            state = parts.next();
        }
        match state {
            Some(state) => {
                rules.states.insert(state.to_owned(), colors);
            }
            None => rules.colors = colors,
        }
    }
    Ok(compiled)
}
const BUTTON_CLASSES: &[&str] = &["primary", "secondary", "success", "danger", "text"];

trait Paint {
    fn keys() -> &'static [&'static str];
    fn paint(&mut self, key: &str, color: iced::Color);
}
fn apply(theme: &Theme, widget: &str, class: Option<&str>, state: &str, style: &mut impl Paint) {
    let Some(rules) = theme
        .overrides
        .as_ref()
        .and_then(|o| o.widgets.0.get(widget))
    else {
        return;
    };
    let class = class.and_then(|class| rules.classes.get(class));
    for colors in [
        Some(&rules.colors),
        rules.states.get(state),
        class.map(|rules| &rules.colors),
        class.and_then(|rules| rules.states.get(state)),
    ]
    .into_iter()
    .flatten()
    {
        for (key, color) in colors {
            style.paint(key, color.0);
        }
    }
}

macro_rules! paint {
    ($ty:ty, $style:ident, $color:ident, $($key:literal => $set:expr),* $(,)?) => {
        impl Paint for $ty {
            fn keys() -> &'static [&'static str] { &[$($key),*] }
            fn paint(&mut self, key: &str, $color: iced::Color) {
                let $style = self;
                match key { $($key => { $set; }),* _ => {} }
            }
        }
    };
}
paint!(text::Style, s, c, "text" => s.color = Some(c));
paint!(button::Style, s, c, "background" => s.background = Some(c.into()), "text" => s.text_color = c, "border" => s.border.color = c, "shadow" => s.shadow.color = c);
paint!(container::Style, s, c, "background" => s.background = Some(c.into()), "text" => s.text_color = Some(c), "border" => s.border.color = c, "shadow" => s.shadow.color = c);
paint!(checkbox::Style, s, c, "background" => s.background = c.into(), "text" => s.text_color = Some(c), "border" => s.border.color = c, "icon" => s.icon_color = c);
paint!(pick_list::Style, s, c, "background" => s.background = c.into(), "text" => s.text_color = c, "border" => s.border.color = c, "placeholder" => s.placeholder_color = c, "handle" => s.handle_color = c);
paint!(text_input::Style, s, c, "background" => s.background = c.into(), "text" => s.value = c, "border" => s.border.color = c, "placeholder" => s.placeholder = c, "icon" => s.icon = c, "selection" => s.selection = c);
paint!(menu::Style, s, c, "background" => s.background = c.into(), "text" => s.text_color = c, "border" => s.border.color = c, "shadow" => s.shadow.color = c, "selected_text" => s.selected_text_color = c, "selected_background" => s.selected_background = c.into());
paint!(progress_bar::Style, s, c, "background" => s.background = c.into(), "bar" => s.bar = c.into(), "border" => s.border.color = c);
paint!(rule::Style, s, c, "color" => s.color = c);
paint!(table::Style, s, c, "separator_x" => s.separator_x = c.into(), "separator_y" => s.separator_y = c.into());
paint!(slider::Style, s, c, "rail_filled" => s.rail.backgrounds.0 = c.into(), "rail_empty" => s.rail.backgrounds.1 = c.into(), "rail_border" => s.rail.border.color = c, "handle" => s.handle.background = c.into(), "handle_border" => s.handle.border_color = c);
paint!(scrollable::Style, s, c,
    "background" => s.container.background = Some(c.into()), "border" => s.container.border.color = c,
    "gap" => s.gap = Some(c.into()),
    "rail" => { s.vertical_rail.background = Some(c.into()); s.horizontal_rail.background = Some(c.into()); },
    "rail_border" => { s.vertical_rail.border.color = c; s.horizontal_rail.border.color = c; },
    "scroller" => { s.vertical_rail.scroller.background = c.into(); s.horizontal_rail.scroller.background = c.into(); },
    "scroller_border" => { s.vertical_rail.scroller.border.color = c; s.horizontal_rail.scroller.border.color = c; },
    "auto_scroll" => s.auto_scroll.background = c.into(), "auto_scroll_icon" => s.auto_scroll.icon = c,
    "auto_scroll_border" => s.auto_scroll.border.color = c, "auto_scroll_shadow" => s.auto_scroll.shadow.color = c,
);

pub(super) fn validate(
    widgets: &std::collections::BTreeMap<String, std::collections::BTreeMap<String, super::Hex>>,
) -> Result<(), String> {
    for (path, colors) in widgets {
        let parts: Vec<_> = path.split('.').collect();
        let widget = parts[0];
        let keys = match widget {
            "text" => text::Style::keys(),
            "button" => button::Style::keys(),
            "container" => container::Style::keys(),
            "checkbox" => checkbox::Style::keys(),
            "pick_list" => pick_list::Style::keys(),
            "text_input" => text_input::Style::keys(),
            "menu" => menu::Style::keys(),
            "progress_bar" => progress_bar::Style::keys(),
            "rule" => rule::Style::keys(),
            "table" => table::Style::keys(),
            "slider" => slider::Style::keys(),
            "scrollable" => scrollable::Style::keys(),
            _ => return Err(format!("unknown widget {widget}")),
        };
        let states: &[&str] = match widget {
            "button" => &["active", "hovered", "pressed", "disabled"],
            "checkbox" => &["active", "hovered", "disabled"],
            "pick_list" => &["active", "hovered", "opened"],
            "text_input" => &["active", "hovered", "focused", "disabled"],
            "slider" | "scrollable" => &["active", "hovered", "dragged"],
            _ => &[],
        };
        let suffix = &parts[1..];
        let valid = suffix.is_empty()
            || (suffix.len() == 1 && states.contains(&suffix[0]))
            || (widget == "button"
                && suffix.len() <= 2
                && BUTTON_CLASSES.contains(&suffix[0])
                && (suffix.len() == 1 || states.contains(&suffix[1])));
        if !valid {
            return Err(format!("unknown widget style {path}"));
        }
        for key in colors.keys() {
            if !keys.contains(&key.as_str()) {
                return Err(format!("unknown widget colour {path}.{key}"));
            }
        }
    }
    Ok(())
}

/// Preserve the original emphasis of sensor groups and navigation group captions.
pub fn section_heading_accent(theme: &Theme) -> text::Style {
    let mut style = text::primary(theme);
    if let Some(color) = section_heading(theme).color {
        style.color = Some(color);
    }
    style
}
pub fn section_heading_muted(theme: &Theme) -> text::Style {
    let mut style = text_secondary(theme);
    if let Some(color) = section_heading(theme).color {
        style.color = Some(color);
    }
    style
}
