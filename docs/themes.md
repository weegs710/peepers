# Themes

Pick a theme on the Settings page. It is saved as `appearance.theme` in [the settings file](./settings.md), by the name the list shows.

## Built-in themes

These come with peepers and need nothing else. Those marked iced are [iced](https://iced.rs)'s own; those marked peepers are palettes that ship with peepers.

| Theme                | From    |
| -------------------- | ------- |
| Light                | iced    |
| Dark                 | iced    |
| Dracula              | iced    |
| Nord                 | iced    |
| Solarized Light      | iced    |
| Solarized Dark       | iced    |
| Gruvbox Light        | iced    |
| Gruvbox Dark         | iced    |
| Catppuccin Latte     | iced    |
| Catppuccin Frappé    | iced    |
| Catppuccin Macchiato | iced    |
| Catppuccin Mocha     | iced    |
| Tokyo Night          | iced    |
| Tokyo Night Storm    | iced    |
| Tokyo Night Light    | iced    |
| Kanagawa Wave        | iced    |
| Kanagawa Dragon      | iced    |
| Kanagawa Lotus       | iced    |
| Moonfly              | iced    |
| Nightfly             | iced    |
| Oxocarbon            | iced    |
| Ferra                | iced    |
| Cold Wind            | peepers |
| Cymagentan           | peepers |
| Great Below          | peepers |
| Plasm                | peepers |
| Triglavian           | peepers |
| Twilight             | peepers |

### The peepers palettes

The six peepers palettes are noctalia custom palette files in `crates/ui/themes/`, and only their dark set is used. peepers draws with six colours, and takes four of them from the palette:

| peepers uses | Palette role  |
| ------------ | ------------- |
| background   | `mSurface`    |
| text         | `mOnSurface`  |
| accent       | `mPrimary`    |
| errors       | `mError`      |

The shipped palettes retain iced's success and warning colours. Custom TOML themes can override all six palette colours and the individual interface and graph colours below.

## Custom TOML themes

Place `*.toml` files in `$XDG_CONFIG_HOME/peepers/themes/` (normally `~/.config/peepers/themes/`). They appear in Settings → Theme as **Custom: filename**, without the `.toml` extension. The filename is the saved identity; it cannot collide with a built-in theme. Files are discovered while Settings is open or a custom theme is selected. The selected file is checked once a second and reloaded when its identity, size or change timestamps differ, including atomic file replacement. Built-in themes do not poll theme files when Settings is closed. No rebuild or restart is needed after editing colours.

Use **Settings → Create theme template** to write `default.toml` into your custom themes directory. The template is embedded in the binary, so this also works with standalone builds. It starts with `base = "Dark"` and all colour overrides commented out, preserving the built-in appearance. The button never overwrites an existing file. Select **Custom: default**, then uncomment and edit the options you want; change `base` to inherit another built-in theme. The completion message shows the file's location.

The installed Nix package also includes `default.toml`, `arcane-neon.toml` and `crazy.toml` under `share/peepers/themes/`, with this guide under `share/doc/peepers/`.

For a colourful starting point, use [`packaging/themes/arcane-neon.toml`](../packaging/themes/arcane-neon.toml). It includes a commented option map for every graph and series, device overrides, interface roles, control states, tray colours and derived palette pairs. Uncomment the table header and the values you want to override; the example colours are suggestions, while omitted values keep inheriting their defaults.

Copy it into your configuration:

```sh
mkdir -p ~/.config/peepers/themes
cp packaging/themes/arcane-neon.toml ~/.config/peepers/themes/
```

Select **Custom: arcane-neon**. An existing Noctalia-format file can also be copied into this directory unchanged. Peepers never rewrites theme files. Missing or invalid selected files fall back to Dark and show the reason in Settings; correcting the file restores the theme automatically. Unknown colour keys are errors, so misspellings are visible.

Every field is optional. Colours accept `#RRGGBB` or `#RRGGBBAA`; alpha `00` makes a graph fill or glow invisible. The starting palette is `base`, the exact name of any built-in theme, or `mode = "dark"` / `"light"` when no base is given. Dark is the default.

```toml
base = "Dark"
background = "#10081F" # also accepts surface, for Noctalia compatibility
text = "#F6E7FF"       # also accepts on_surface
primary = "#FF39C8"
success = "#B66BFF"
warning = "#49E9FF"
danger = "#FF547D"     # also accepts error
```

These six colours generate the default control, text and graph colours. Setting `success` and `warning` changes status indicators as well as the corresponding graph series. Use graph overrides to change only graphs.

### Interface roles

```toml
[ui]
heading = "#FF39C8"
section_heading = "#DCA6FF"
table_heading = "#DCA6FF"
secondary_text = "#BCA6CF"
label = "#BCA6CF"
border = "#513166"
```

Supported roles:

| Keys | Used for |
| --- | --- |
| `heading`, `section_heading`, `table_heading` | Page/detail titles, section/card titles, sortable column headings and arrows. Section and table headings inherit `heading` when omitted. |
| `label`, `value` | Captions and values in statistics blocks; values in device cards. |
| `secondary_text`, `success_text`, `danger_text` | Muted text and explicit status text. |
| `icon`, `selected_icon` | Navigation icons. |
| `row_hover`, `row_selected` | Hovered and selected table rows. |
| `panel`, `panel_text`, `border` | Rounded/bordered panels and their borders. |
| `tooltip`, `tooltip_text` | Graph tooltip background and text. |
| `separator` | Table separators and rules. |

### Graphs and individual lines

`[graph]` provides defaults for all graphs. `[graphs.KEY]` overrides one graph family; `[graphs.KEY.instances.ID]` overrides one device, core or entity. Inside any of these tables, `[...lines.LINE]` overrides a single series. Quote keys containing dots or device punctuation.

```toml
[graph]
primary = "#FF39C8"
success = "#B66BFF"
warning = "#49E9FF"
grid = "#51316680"
axis = "#BCA6CF"

[graphs.network.lines.receive]
color = "#B66BFF"

[graphs.network.lines.send]
color = "#49E9FF"
fill = "#49E9FF20"

[graphs.network.instances.eth0.lines.receive]
color = "#FF547D"

[graphs."cpu.core".instances."0".lines.utilization]
color = "#49E9FF"
```

Graph colour keys:

| Key | Used for |
| --- | --- |
| `background` | Plot background; transparent by default. |
| `grid`, `axis` | Horizontal gridlines and scale labels. |
| `legend`, `tooltip` | Tooltip text and background. |
| `crosshair`, `marker` | Hover cursor and flight recorder playback cursor. |
| `primary`, `success`, `warning` | Defaults for the three series roles. |
| `color` | Base colour for all lines in this scope. |
| `stroke`, `fill`, `glow`, `point` | Line stroke, area beneath it, glow and hover dots. |

Line tables accept `color`, `stroke`, `fill`, `glow`, `point`, `primary`, `success`, `warning`. A configured `color` is used exactly for the stroke and dots, with translucent fill and glow derived from it. Individual part overrides take priority over the derived colour. Omitted colours retain the built-in appearance, including its slightly lighter strokes. Legend dots follow the line stroke.

Resolution is per colour: instance line → instance graph → family line → family graph → global line → global graph → generated palette. This lets a specific graph override a global series colour without changing other graphs. Graph rendering caches include resolved colours, so editing only a line or grid refreshes immediately.

| Graph key | Line keys | Instance identifier |
| --- | --- | --- |
| `cpu` | `utilization` | — |
| `cpu.core` | `utilization` | Logical processor index, starting at `0` |
| `memory` | `used` | — |
| `disk.activity` | `active` | Disk name, e.g. `nvme0n1` |
| `disk.io` | `read`, `write` | Disk name |
| `network` | `receive`, `send` | Interface name, e.g. `eth0` |
| `gpu` | `utilization`, `memory` | Card name, e.g. `card0` |
| `clock` | `frequency` | — |
| `sensor` | `value` | Chip ID followed by `/` and reading label |
| `pressure` | `stalled` | `cpu`, `memory`, or `io` |
| `energy`, `energy.activity` | `value` | Energy report entity ID |
| `flight.cpu` | `utilization` | — |
| `flight.memory` | `used` | — |
| `flight.disk` | `read`, `write` | Recorded disk name |
| `flight.network` | `receive`, `send` | Recorded interface name |
| `flight.gpu` | `utilization`, `memory` | Recorded GPU name |
| `flight.sensor` | `value` | Recorded sensor label |

Performance cards, detail views and the popout share their graph keys. Flight recorder graphs have their own keys.

### Control colours and interaction states

For more precise control, `[widgets.WIDGET]` sets colours for a control type. Add a state or a button style to narrow it down. The order is widget → widget state → button style → button style state.

```toml
[widgets.text_input]
background = "#160D27"
text = "#F6E7FF"
placeholder = "#BCA6CF"

[widgets."text_input.focused"]
border = "#FF39C8"
selection = "#513166"

[widgets."button.primary.hovered"]
background = "#D52DA7"
text = "#10081F"
```

| Widget | Colour keys | States |
| --- | --- | --- |
| `text` | `text` | — |
| `button` | `background`, `text`, `border`, `shadow` | `active`, `hovered`, `pressed`, `disabled` |
| `container` | `background`, `text`, `border`, `shadow` | — |
| `checkbox` | `background`, `text`, `border`, `icon` | `active`, `hovered`, `disabled` |
| `pick_list` | `background`, `text`, `border`, `placeholder`, `handle` | `active`, `hovered`, `opened` |
| `text_input` | `background`, `text`, `border`, `placeholder`, `icon`, `selection` | `active`, `hovered`, `focused`, `disabled` |
| `menu` | `background`, `text`, `border`, `shadow`, `selected_text`, `selected_background` | — |
| `progress_bar` | `background`, `bar`, `border` | — |
| `slider` | `rail_filled`, `rail_empty`, `rail_border`, `handle`, `handle_border` | `active`, `hovered`, `dragged` |
| `scrollable` | `background`, `border`, `gap`, `rail`, `rail_border`, `scroller`, `scroller_border`, `auto_scroll`, `auto_scroll_icon`, `auto_scroll_border`, `auto_scroll_shadow` | `active`, `hovered`, `dragged` |
| `table` | `separator_x`, `separator_y` | — |
| `rule` | `color` | — |

Button styles are `primary`, `secondary`, `success`, `danger`, and `text`. For example, `[widgets."button.danger"]` changes destructive buttons without affecting other buttons. Widget overrides affect colours, preserving the controls' normal layout and behaviour. Explicit text roles in `[ui]` take precedence over `[widgets.text]`.

### Tray miniature graphs

The tray's CPU and memory graphs follow the palette too: background and text use those palette roles, and low/medium/high load use success/warning/danger. Override both graphs in `[tray]`, or just one in `[tray.cpu]` or `[tray.memory]`:

```toml
[tray]
background = "#10081F"
text = "#F6E7FF"
low = "#B66BFF"
medium = "#49E9FF"
high = "#FF547D"

[tray.cpu]
low = "#FF39C8"
```

All five keys are available in each table. The thresholds remain below 60%, below 85%, and 85% or higher. Tray colours are opaque (alpha is ignored); the gap between the two panels remains transparent. Existing history is recoloured on theme changes. Tray menu styling is controlled by the desktop's tray host.

### Derived palette pairs

For a coherent change across several controls, the generated palette can also be overridden directly:

```toml
[extended.background.weak]
color = "#1D1030"
text = "#F6E7FF"

[extended.primary.strong]
color = "#FF39C8"
text = "#10081F"
```

Families are `background`, `primary`, `secondary`, `success`, `warning`, `danger`. All have `base`, `weak`, `strong` pairs; background also has `weakest`, `weaker`, `neutral`, `stronger`, `strongest`. Each pair accepts `color` and `text`. These are exact overrides after palette generation; specific interface, graph and widget overrides take precedence.

## Noctalia

The Noctalia theme follows the palette [noctalia](https://noctalia.dev) is using. noctalia hands its colours to other programs through templates, so peepers ships one: [`packaging/noctalia/colors.toml`](../packaging/noctalia/colors.toml). noctalia fills it in and writes the result to `$XDG_CONFIG_HOME/peepers/noctalia.toml`, and does so again whenever its palette changes.

While Noctalia is the chosen theme, peepers looks at that file once a second and reads it again when it has changed, so a new palette shows up without a restart. The existing template provides `surface`, `on_surface`, `primary` and `error`. The loader also accepts optional `success` and `warning` and all the custom-theme tables documented above. Omitted status colours follow the light or dark mode. Put persistent manual customizations in a custom theme file, since Noctalia may regenerate its output.

Until the file exists, or when it cannot be read, peepers draws with Dark and the Settings page says why.

### Adding the template

Any `*.toml` in noctalia's configuration folder is read as part of its configuration, so one small file adds the template. Save it as `~/.config/noctalia/peepers.toml`, with the path to your copy of peepers:

```toml
[theme.templates.user.peepers]
input_path  = "/path/to/peepers/packaging/noctalia/colors.toml"
output_path = "$XDG_CONFIG_HOME/peepers/noctalia.toml"
```

Give `input_path` in full. In `output_path`, noctalia expands `$XDG_CONFIG_HOME` and `~`, but not `$HOME`.

To check what the template produces without changing anything, render it against a palette by hand:

```console
$ noctalia theme --dark --theme-json tokens.json -r packaging/noctalia/colors.toml:/tmp/peepers-noctalia.toml
```

`tokens.json` is a flat map of noctalia's colour names to `#rrggbb` values.

[`packaging/noctalia/template.toml`](../packaging/noctalia/template.toml) describes the same template in the shape noctalia's community template catalog uses.
