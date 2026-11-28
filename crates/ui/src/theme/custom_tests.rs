use super::*;
use iced::widget::{button, pick_list, text_input};

fn color(value: &str) -> Color {
    value.parse().unwrap()
}

#[test]
fn partial_files_inherit_the_base_and_status_colors_are_optional() {
    let theme = parse(
        "base = 'Nord'\nsuccess = '#B66BFF'\nwarning = '#49E9FF'",
        "Custom: neon",
    )
    .unwrap();
    assert_eq!(
        theme.palette().background,
        iced::Theme::Nord.palette().background
    );
    assert_eq!(theme.palette().success, color("#B66BFF"));
    assert_eq!(theme.palette().warning, color("#49E9FF"));
    assert_eq!(
        parse("mode = 'light'", "Light file").unwrap().palette(),
        Palette::LIGHT
    );
    assert_eq!(parse("", "Default file").unwrap().palette(), Palette::DARK);
}

#[test]
fn graph_colors_cascade_from_instance_to_graph_to_defaults() {
    let theme = parse(
        r##"
[graph]
color = "#FF39C8"
axis = "#F6E7FF"
[graphs.network]
success = "#B66BFF"
[graphs.network.lines.send]
color = "#49E9FF"
[graphs.network.instances.eth0.lines.receive]
color = "#FF547D"
"##,
        "Neon",
    )
    .unwrap();
    let receive = |instance| {
        theme.graph_colors(
            "network",
            Some(instance),
            Some("receive"),
            &["color", "success"],
        )
    };
    assert_eq!(receive("eth0"), Some(color("#FF547D")));
    assert_eq!(receive("wlan0"), Some(color("#B66BFF")));
    assert_eq!(
        theme.graph_color("network", Some("eth0"), Some("send"), "color"),
        Some(color("#49E9FF"))
    );
    assert_eq!(
        theme.graph_color("cpu", None, Some("utilization"), "color"),
        Some(color("#FF39C8"))
    );
    assert_eq!(
        theme.graph_color("network", Some("eth0"), None, "axis"),
        Some(color("#F6E7FF"))
    );
}

#[test]
fn controls_and_headings_use_exact_overrides_with_state_fallbacks() {
    let theme = parse(
        r##"
[ui]
heading = "#F6E7FF"
section_heading = "#FF39C8"
[extended.primary.strong]
color = "#B66BFF"
text = "#10081F"
[widgets.button]
text = "#49E9FF"
[widgets."button.primary.hovered"]
background = "#FF547D"
[widgets."text_input.focused"]
selection = "#B66BFF"
[widgets."pick_list.opened"]
handle = "#49E9FF"
"##,
        "Neon",
    )
    .unwrap();
    assert_eq!(styles::heading(&theme).color, Some(color("#F6E7FF")));
    assert_eq!(
        styles::section_heading(&theme).color,
        Some(color("#FF39C8"))
    );
    assert_eq!(
        theme.extended_palette().primary.strong.text,
        color("#10081F")
    );
    let active = styles::button_primary(&theme, button::Status::Active);
    let hovered = styles::button_primary(&theme, button::Status::Hovered);
    assert_eq!(active.text_color, color("#49E9FF"));
    assert_eq!(hovered.background, Some(color("#FF547D").into()));
    let class = <Theme as text_input::Catalog>::default();
    let input = text_input::Catalog::style(
        &theme,
        &class,
        text_input::Status::Focused { is_hovered: false },
    );
    assert_eq!(input.selection, color("#B66BFF"));
    let class = <Theme as pick_list::Catalog>::default();
    let pick = pick_list::Catalog::style(
        &theme,
        &class,
        pick_list::Status::Opened { is_hovered: false },
    );
    assert_eq!(pick.handle_color, color("#49E9FF"));
}

#[test]
fn misspelled_roles_and_invalid_colors_are_reported() {
    for contents in [
        "succes = '#ffffff'",
        "success = 'pinkish'",
        "base = 'unknown'",
        "[ui]\nheadng = '#ffffff'",
        "[graphs.netwrok]\ncolor = '#ffffff'",
        "[graphs.network.lines.recieve]\ncolor = '#ffffff'",
        "[graph]\ngird = '#ffffff'",
        "[graphs.cpu.lines.utilization]\nstoke = '#ffffff'",
        "[extended.background.missing]\ncolor = '#ffffff'",
        "[widgets.button]\ntxt = '#ffffff'",
        "[widgets.'button.focused']\ntext = '#ffffff'",
    ] {
        assert!(parse(contents, "Invalid").is_err(), "accepted {contents}");
    }
}

#[test]
fn shipped_example_is_valid_and_uses_arcane_neon_colors() {
    let theme = parse(
        include_str!("../../../../packaging/themes/arcane-neon.toml"),
        "Arcane Neon",
    )
    .unwrap();
    assert_eq!(theme.palette().background, color("#10081F"));
    assert_eq!(theme.palette().success, color("#B66BFF"));
}

#[test]
fn tray_colors_inherit_the_palette_and_allow_independent_panels() {
    let theme = parse(
        "success = '#B66BFF'\n[tray]\ntext = '#F6E7FF'\n[tray.cpu]\nlow = '#FF39C8'",
        "Tray",
    )
    .unwrap();
    let colors = theme.tray_colors();
    assert_eq!(colors.cpu.low, [255, 57, 200]);
    assert_eq!(colors.memory.low, [182, 107, 255]);
    assert_eq!(colors.cpu.text, [246, 231, 255]);
    assert_eq!(colors.memory.text, [246, 231, 255]);
    assert!(parse("[tray.cpu]\nlwo = '#ffffff'", "Invalid").is_err());
}

#[test]
fn circus_theme_loads_with_all_overrides() {
    let theme = parse(
        include_str!("../../../../packaging/themes/crazy.toml"),
        "Custom: crazy",
    )
    .unwrap();
    assert!(
        theme
            .graph_color("cpu.core", Some("0"), Some("utilization"), "stroke")
            .is_some()
    );
    assert!(theme.override_color("section_heading").is_some());
}

#[test]
fn widget_rules_keep_generic_state_class_and_class_state_precedence() {
    let theme = parse(
        r##"
[widgets.button]
background = "#100001"
text = "#100002"
[widgets."button.hovered"]
background = "#200001"
text = "#200002"
[widgets."button.primary"]
background = "#300001"
[widgets."button.primary.hovered"]
text = "#400002"
"##,
        "Layered",
    )
    .unwrap();
    let primary = styles::button_primary(&theme, button::Status::Hovered);
    assert_eq!(primary.background, Some(color("#300001").into()));
    assert_eq!(primary.text_color, color("#400002"));
    let secondary = styles::button_secondary(&theme, button::Status::Hovered);
    assert_eq!(secondary.background, Some(color("#200001").into()));
    assert_eq!(secondary.text_color, color("#200002"));
    let active = styles::button_secondary(&theme, button::Status::Active);
    assert_eq!(active.background, Some(color("#100001").into()));
}

#[test]
fn bundled_template_inherits_dark_and_all_commented_examples_are_valid() {
    let theme = parse(TEMPLATE, "Default template").unwrap();
    assert_eq!(theme.palette(), Theme::Dark.palette());
    assert_eq!(theme.extended_palette(), Theme::Dark.extended_palette());
    assert_eq!(
        styles::button_primary(&theme, button::Status::Hovered),
        styles::button_primary(&Theme::Dark, button::Status::Hovered)
    );
    let enabled = TEMPLATE
        .lines()
        .map(|line| {
            let Some(example) = line.strip_prefix("# ") else {
                return line;
            };
            if (example.starts_with('[') && example.ends_with(']'))
                || example.split_once(" = \"").is_some_and(|(key, _)| {
                    key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                })
            {
                example
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    parse(&enabled, "All examples").expect("every documented table and colour must be accepted");
}
