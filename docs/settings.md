# The settings file

`settings.toml` in `$XDG_CONFIG_HOME/peepers`, or in `~/.config/peepers` when that is not set. The Settings page shows the path.

A missing file means the defaults. peepers makes the file the first time a setting is changed.

```toml
[appearance]
theme = "Dark"
text_size = 16

[startup]
page = "processes"
tray = true

[sampling]
interval = "1s"

[units]
temperature = "celsius"
```

| Key                      | Values                                                          | Default     |
| ------------------------ | --------------------------------------------------------------- | ----------- |
| `appearance.theme`       | any theme iced lists, by the name the Settings page shows       | `Dark`      |
| `appearance.text_size`   | 12 to 24 in steps of 2. Any other value is moved to the nearest | `16`        |
| `startup.page`           | a page name from [Controlling a running peepers](./control.md#pages) | `processes` |
| `startup.tray`           | `true` or `false`                                               | `true`      |
| `sampling.interval`      | `100ms`, `250ms`, `500ms`, `1s`, `2s` or `5s`                   | `1s`        |
| `units.temperature`      | `celsius` or `fahrenheit`                                       | `celsius`   |

## Reading

peepers reads the file when it starts. A key it does not know is ignored. A file that cannot be read, or that has a value of the wrong kind, leaves the defaults in use and the Settings page says why.

A `settings.json` from an earlier version is not read.

## Saving

A change in the window is saved as it is made. Only the settings that were changed are written. The rest of the file stays as it is: its comments, the keys peepers does not know, and anything edited by hand since peepers started.

- A file that is not valid TOML is not changed, and the Settings page says the change was not saved.
- A file that is a symbolic link is written through the link, and stays a link.
- The new contents are written to a temporary file beside the target and renamed into place.
