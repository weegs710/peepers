# Controlling a running peepers

The first peepers to start owns the name `dev.weegs.Peepers` on the session bus. The command line options and any other D-Bus caller talk to it there. A second copy started with no option cannot take the name from the first, so it runs on its own and cannot be controlled this way.

## Options

| Option                 | Does                                                  | Method           |
| ---------------------- | ----------------------------------------------------- | ---------------- |
| `--toggle-pop`         | opens the popout, or closes it                        | `TogglePopout`   |
| `--show-pop`           | opens the popout unless it is open                    | `ShowPopout`     |
| `--hide-pop`           | closes the popout if it is open                       | `HidePopout`     |
| `--toggle-window`      | shows the main window, or hides it                    | `ToggleWindow`   |
| `--show-window`        | opens the main window unless it is open               | `ShowWindow`     |
| `--hide-window`        | closes the main window if it is open                  | `HideWindow`     |
| `--toggle-pause`       | holds the numbers still, or lets them move again      | `TogglePause`    |
| `--pause`              | holds the numbers still                               | `Pause`          |
| `--resume`             | lets the numbers move again                           | `Resume`         |
| `--page NAME`          | shows a page, opening the window if it is closed      | `OpenPage`       |
| `--flight start`       | starts a flight recording                             | `StartRecording` |
| `--flight stop`        | stops it and saves it                                 | `StopRecording`  |
| `--quit`               | ends the application, asking first if a recording runs | `Quit`          |
| `--quit save`          | ends it, saving a running recording first             | `QuitSaving`     |
| `--quit discard`       | ends it, dropping a running recording                 | `QuitDiscarding` |

Each toggle has explicit counterparts: `--show-pop` and `--hide-pop`, `--show-window` and `--hide-window`, and `--pause` and `--resume`. Unlike a toggle, they give the same result however many times they are sent, which suits a script or an alert handler that may run more than once. A toggle or a show sent while a window is still closing reopens it.

The exit status is `0` when the running copy accepted the request, `1` when no copy could be reached or it refused, and `2` when the options themselves do not make sense, which includes a page that does not exist. The reason is printed to stderr, followed by the usage when the options were the problem.

### What each one does

- **Hiding the main window** leaves the application running. The popout, the tray icon and the command line still work, and `--show-window`, `--toggle-window` or `--page` brings the window back. Without a tray icon or a popout, nothing is left on screen, and the command line is the way back.
- **`--page`** switches to the page, and opens the main window if it is closed or brings it forward if it is open without the focus. A window that already has the focus is left alone.
- **Pause** freezes what the pages and graphs show. The tray gauges and a running flight recording keep updating.
- **Quitting with a recording running** never loses it silently:
  - A plain `--quit` does not end the application. A bar across the main window asks whether to save the recording, discard it or keep running, and the call fails with exit status 1 so a script can tell the application did not end.
  - `--quit save` stops the recording, writes the file and ends once the file is written. If the file cannot be written, the application stays open and says why.
  - `--quit discard` ends at once and keeps nothing.
  - Quitting from the tray, or closing the last window when there is no tray icon, asks the same way.
  - The call returns once the request is accepted, not once the process has exited.
- **`--flight start`** works with no window open. It needs a reading to begin from, so it does nothing in the first moments after the application starts. It also does nothing when a recording is already running, and `--flight stop` does nothing when none is. A stopped recording is saved to the folder set on the Flight recorder page, under the name typed there. A recording ends by itself when it is full: see [Flight recorder](./features.md#flight-recorder) for how long that is.

### Pages

`--page` takes the name in the first column.

| Name          | Page               |
| ------------- | ------------------ |
| `processes`   | Processes          |
| `performance` | Performance        |
| `pressure`    | Pressure           |
| `power`       | Power              |
| `flight`      | Flight recorder    |
| `benchmarks`  | Benchmarks         |
| `connections` | Connections        |
| `drivers`     | Drivers            |
| `disk-space`  | Disk space         |
| `apps`        | Installed apps     |
| `info`        | System information |
| `services`    | Services           |
| `startup`     | Startup apps       |
| `users`       | Users              |
| `settings`    | Settings           |

## D-Bus

| Part      | Value               |
| --------- | ------------------- |
| Bus       | the session bus     |
| Name      | `dev.weegs.Peepers` |
| Object    | `/dev/weegs/Peepers` |
| Interface | `dev.weegs.Peepers` |

### Methods

Every method takes no arguments and returns nothing, except `OpenPage`, which takes the page name as a string.

```
TogglePopout   ShowPopout   HidePopout
ToggleWindow   ShowWindow   HideWindow
TogglePause   Pause   Resume
StartRecording   StopRecording   OpenPage(s)
Quit   QuitSaving   QuitDiscarding
```

A page name that does not exist is refused with an `InvalidArgs` error whose message lists the names that do.

### Properties

Properties describe the running application's state. They can be read but not set, and the application updates them as its state changes.

| Property      | Value                                                        |
| ------------- | ------------------------------------------------------------ |
| `Paused`      | true or false: the pages are holding what they show          |
| `PopoutOpen`  | true or false: the popout window is open                     |
| `WindowOpen`  | true or false: the main window is open                       |
| `Page`        | text: the page the main window is on, as `--page` names it   |
| `Recording`   | true or false: a flight recording is running                 |

In D-Bus terms the true-or-false properties are booleans (type `b`) and `Page` is a string (type `s`). `Page` is empty until the application has reported for the first time.

When a property changes, the application sends the standard `org.freedesktop.DBus.Properties.PropertiesChanged` signal with only the properties that changed, so a script that listens for it learns about every change without asking again.

### From a shell

`busctl`, which comes with systemd, can do everything the command line options do and more. Each command below names the bus name, the object and the interface from the table above.

Call a method. This one shows the Services page; `s` says the argument that follows is a string:

```console
$ busctl --user call dev.weegs.Peepers /dev/weegs/Peepers \
    dev.weegs.Peepers OpenPage s services
```

Read a property. This prints `b true` while the numbers are paused and `b false` otherwise:

```console
$ busctl --user get-property dev.weegs.Peepers /dev/weegs/Peepers \
    dev.weegs.Peepers Paused
```

Watch everything peepers sends and receives on the bus, including each `PropertiesChanged`, until you press Ctrl+C:

```console
$ busctl --user monitor dev.weegs.Peepers
```
