# peepers

A system monitor and task manager for Linux, written in Rust with [iced](https://iced.rs). One window of pages for watching, recording, inspecting and managing the machine, a popout of live gauges, a tray icon, and a command line that drives the running copy.

> **Important**: This is a work in progress and only exists as a development build for now. Changes will keep coming in and they may be breaking, and some features and functions may be dropped in future iterations. Treat the codebase as an alpha. Backwards compatibility is not currently on the table, but it is a consideration being made in the development cycle moving forward.

> **Requirements**: Linux, a Rust toolchain that supports edition 2024, and the libraries in [`shell.nix`](./shell.nix): wayland, libxkbcommon and vulkan-loader. The tray icon and the command line need a session D-Bus. See [Platforms](./docs/platforms.md) for everything else it reads.

## Quickstart

```console
$ nix-shell                       # the libraries and the build directory
$ cargo build --release
$ peepers                         # the window
$ peepers --toggle-pop            # from another terminal: open or close the popout
$ peepers --page services         # show a page, opening the window if it is closed
$ peepers --flight start          # start a flight recording, no window needed
```

`peepers` is `$CARGO_TARGET_DIR/release/peepers` until it is installed somewhere on your `PATH`. Inside `nix-shell` that variable is `~/.cache/cargo-target/peepers`.

## What It Shows

| Section | Pages                                                                    |
| ------- | ------------------------------------------------------------------------ |
| WATCH   | Processes, Performance, Pressure, Power                                  |
| RECORD  | Flight recorder, Benchmarks                                              |
| INSPECT | Connections, Drivers, Disk space, Installed apps, System information     |
| MANAGE  | Services, Startup apps, Users                                            |
| PEEPERS | Settings                                                                 |

The popout is a second, narrow window of the same live gauges that stays beside the work. It has its own application id, `peepers-popout`, so a compositor rule can float and place it. The main window's is `peepers`.

**Documentation:** [Features](./docs/features.md) ·
[Controlling a running peepers](./docs/control.md) ·
[The settings file](./docs/settings.md) ·
[Architecture](./docs/architecture.md) ·
[Platforms](./docs/platforms.md) ·
[Building and testing](./docs/building.md)

## Command line

With no option peepers starts the application. With one it asks the copy that is already running to act, and exits:

```
--toggle-pop          open the popout, or close it
--show-pop            open the popout unless it is open
--hide-pop            close the popout if it is open
--toggle-window       show the main window, or hide it
--show-window         open the main window unless it is open
--hide-window         close the main window if it is open
--toggle-pause        hold the numbers still, or let them move again
--pause               hold the numbers still
--resume              let the numbers move again
--page NAME           show a page, opening the window if it is closed
--flight start|stop   start or stop the flight recorder
--quit                end the application, asking about a recording that is running
--quit save           end it, saving a recording that is running first
--quit discard        end it, dropping a recording that is running
```

The exit status is `0` when the running copy accepted the request, `1` when none could be reached or it refused (the reason is printed), and `2` when the options do not make sense, which includes a page that does not exist.

```console
$ peepers --page nonsense
No page called nonsense. Pages: processes, performance, pressure, power, flight, benchmarks, connections, drivers, disk-space, apps, info, services, startup, users, settings
Usage: peepers [OPTION]
...
$ echo $?
2
```

The same requests are D-Bus methods, and the application's state is readable as D-Bus properties, so a script or a compositor key can use either. See [Controlling a running peepers](./docs/control.md).

## Build

```console
$ nix-shell
$ cargo build --release
$ cargo test --workspace
```

See [Building and testing](./docs/building.md) for the tests that need a session bus.
