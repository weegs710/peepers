# Themes

Pick a theme on the Settings page. It is saved as `appearance.theme` in [the settings file](./settings.md), by the name the list shows.

| Themes                                                                 | Colours from                                       |
| ---------------------------------------------------------------------- | -------------------------------------------------- |
| every theme iced ships, from Light and Dark to Ferra                   | iced                                               |
| Cold Wind, Cymagentan, Great Below, Plasm, Triglavian, Twilight        | palettes shipped in `crates/ui/themes/`            |
| Noctalia                                                               | the noctalia shell's current palette, kept in step |

## The shipped palettes

They are noctalia custom palette files, kept as they were written, and only their dark set is used. peepers draws with six colours, and takes four of them from the palette:

| peepers uses | Palette role  |
| ------------ | ------------- |
| background   | `mSurface`    |
| text         | `mOnSurface`  |
| accent       | `mPrimary`    |
| errors       | `mError`      |

The other two, the green for good news and the amber for warnings, are iced's own and the same in every theme, so a status reads the same whatever the palette.

## Noctalia

The Noctalia theme follows the palette [noctalia](https://noctalia.dev) is using. noctalia hands its colours to other programs through templates, so peepers ships one: [`packaging/noctalia/colors.toml`](../packaging/noctalia/colors.toml). noctalia fills it in and writes the result to `$XDG_CONFIG_HOME/peepers/noctalia.toml`, and does so again whenever its palette changes.

While Noctalia is the chosen theme, peepers looks at that file once a second and reads it again when it has changed, so a new palette shows up without a restart. It takes `surface`, `on_surface`, `primary` and `error` from it, the same four roles as the shipped palettes, and iced's light status colours instead of the dark ones when noctalia is in light mode.

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
