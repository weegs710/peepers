# Architecture

Two crates in one workspace.

| Crate          | What it is                                                                                  |
| -------------- | ------------------------------------------------------------------------------------------- |
| `peepers-core` | a library: everything that reads the machine, plus the D-Bus code. It does not depend on iced |
| `peepers`      | the application. An iced daemon that draws the pages, owns the windows and runs the tray     |

The workspace forbids `unsafe` code and builds with clippy's pedantic lints as warnings.

## The readings

`peepers_core::telemetry::Sampler` takes one `Sample`: the `Vitals` and the process list.

| Part of `Vitals` | Holds                                                            |
| ---------------- | ---------------------------------------------------------------- |
| `uptime`, `load` | how long the machine has run, and the scheduler's load           |
| `cpu`, `memory`  | processor usage with each core, and memory and swap              |
| `pressure`       | pressure stall information for processor, memory and I/O         |
| `disks`          | each block device's throughput, activity and response time       |
| `interfaces`     | each network interface's state and rates                         |
| `adapters`       | each graphics adapter's load, memory and sensors                 |
| `chips`          | every hwmon sensor chip                                          |
| `power`          | each power domain's draw, where it can be read                   |

The process list is kept out of `Vitals` because it is large and a history has no use for it.

The application samples on a thread of its own, once per interval regardless of how long a reading takes. The interval is a setting and defaults to one second. Each `Sample` reaches the interface as a `Message::Sampled`. A sensor read that takes milliseconds, such as a drive or graphics query, is cached and read again at most about once a second.

Anything else that can block, such as listing a folder or asking the service manager, goes through `blocking::run`, which runs it on a thread meant for blocking calls so drawing is never held up.

## The application

`Peepers` holds the state of every page, and a `Message` says what happened. `update` applies a message and returns the task it asks for, `view` draws a window, and `subscription` lists what the application is listening to:

- the sampler,
- window events and the zoom keys,
- the command door,
- the tray,
- and whatever the page in view needs, such as the socket tables on Connections or the service list on Services.

A page with readings of its own loads them when it is entered and keeps them current only while it is in view.

A sample feeds the flight recorder and the tray icon whether or not a window is open. The pages' history takes a sample only while a window is open and the numbers are not paused.

The sampling thread also owns `energy::Collector`. It accounts for each successful sample before sending it to the interface, keeps hourly aggregates, and checkpoints them to the XDG state directory. A stable subscription control changes sampling cadence and report range without restarting the thread. Energy reports reach the UI every ten seconds or on a range change. The collector tracks monotonic time, wall time and uptime to leave gaps across failures, suspend and clock changes. Shutdown waits for a checkpoint; failed saves are visible and offer a retry or explicit discard. See [Energy history](./energy.md) for attribution, retention and permissions.

### Windows

`Windows` keeps the main window and the popout. Each has its own application id, `peepers` and `peepers-popout`, which is what a compositor rule matches on. Closing a window destroys it, and opening it again creates a new one. A window that is open without the focus is closed and reopened when it is asked to come forward, because asking an existing window to take the focus had no effect under Umbriel.

A close request on the main window hides it when the tray icon exists. Without the icon it ends the application, because nothing would be left to bring the window back.

### Tray

A StatusNotifierItem on the session bus, registered with `org.kde.StatusNotifierWatcher`. It starts only when a watcher answers. The icon is drawn in code at 22 pixels, which a tray that draws at that size shows without scaling, and again at 44 for a host that draws it larger. Its menu has Window, Popout, Pause and Quit.

### Command door

`peepers_core::ipc::Listener` owns `dev.weegs.Peepers` on the session bus and serves the object that [Controlling a running peepers](./control.md) describes.

- A method call becomes a `Command` sent down a channel. A subscription waits on that channel and hands each command to `Peepers::run`, which turns it into the messages the buttons already send.
- A page name is checked where the call arrives, so a name that does not exist is refused at the bus and never reaches the application.
- What other processes can read is a `State`. `Door::publish` sends it after every update that changed it. A thread owned by the listener writes it to the properties and emits `PropertiesChanged`, so a slow bus never holds up the interface.
- The bus name is requested so that a second claim fails instead of replacing the first.

## Settings

`settings.toml` in `$XDG_CONFIG_HOME/peepers`, or `~/.config/peepers` when that is not set. Each change is written as it is made: the file is read again, the settings that changed since peepers last read or wrote it are set in place, and the result is renamed into place from a temporary file. A file that cannot be read leaves the defaults and says why on the Settings page. [The settings file](./settings.md) lists the keys.
