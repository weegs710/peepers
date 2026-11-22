# peepers

A system monitor and task manager for Linux, written in Rust with [iced](https://iced.rs). One window of pages for watching, recording, inspecting and managing the machine, a popout of live gauges, a tray icon, and a command line that drives the running copy.

> **Important**: This is a work in progress and only exists as a development build for now. Changes will keep coming in and they may be breaking, and some features and functions may be dropped in future iterations. Treat the codebase as an alpha. Backwards compatibility is not currently on the table, but it is a consideration being made in the development cycle moving forward.

> **Requirements**: Linux, a Rust toolchain that supports edition 2024, and three libraries: wayland, libxkbcommon and vulkan-loader. The tray icon and the command line need a session D-Bus, and the Services and Users pages need systemd. See [Platforms](./docs/platforms.md) for everything it reads.

## Quickstart

```console
$ git clone https://github.com/weegs710/peepers.git && cd peepers
$ nix-shell                       # the libraries, or install them yourself
$ cargo build --release
$ peepers                         # the window
$ peepers --toggle-pop            # from another terminal: open or close the popout
$ peepers --page services         # show a page, opening the window if it is closed
$ peepers --flight start          # start a flight recording, no window needed
```

`peepers` is `target/release/peepers` until you put it on your `PATH`. Inside `nix-shell` the build goes to `~/.cache/cargo-target/peepers` instead. See [Install](#install).

## What It Shows

| Section | Pages                                                                    |
| ------- | ------------------------------------------------------------------------ |
| WATCH   | Processes, Performance, Pressure, Power                                  |
| RECORD  | Flight recorder, Benchmarks                                              |
| INSPECT | Connections, Drivers, Disk space, Installed apps, System information     |
| MANAGE  | Services, Startup apps, Users                                            |
| PEEPERS | Settings                                                                 |

The popout is a second, narrow window of the same live gauges that stays beside your work. It has its own application id, `peepers-popout`, so a compositor rule can float and place it. The main window's is `peepers`.

## Themes

Every theme iced ships, six of its own (Cold Wind, Cymagentan, Great Below, Plasm, Triglavian and Twilight), and **Noctalia**, which follows the palette of the [noctalia](https://noctalia.dev) shell and changes when it does. Noctalia needs one template added to noctalia first. See [Themes](./docs/themes.md).

**Documentation:** [Features](./docs/features.md) ·
[Themes](./docs/themes.md) ·
[Energy history](./docs/energy.md) ·
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

## Install

There is no package yet, so peepers is built from source.

**With Nix**, `nix-shell` provides `pkg-config` and the three libraries and points `LD_LIBRARY_PATH` at them:

```console
$ nix-shell --run 'cargo build --release'
$ nix-shell --run ~/.cache/cargo-target/peepers/release/peepers
```

On NixOS the binary only finds its libraries from inside that shell, so start it the same way from a launcher, a key binding or a service.

**Without Nix**, install a Rust toolchain, `pkg-config`, and your distribution's packages for wayland, libxkbcommon and the Vulkan loader, then:

```console
$ cargo build --release
$ install -Dm755 target/release/peepers ~/.local/bin/peepers
```

Only one copy can be driven from the command line, so start it once per session, for example from your compositor's autostart. See [Building and testing](./docs/building.md) for the tests.

## Contributing

Fork it and do whatever. Bug fixes and improvements welcome.

Run `cargo fmt --all`, `cargo clippy --workspace --all-targets` and `cargo test --workspace` before sending anything. Tests that need a session bus are ignored by default, and [Building and testing](./docs/building.md#tests-that-need-a-session-bus) says how to run them.

## License

MIT, please see [LICENSE](LICENSE).

## Links

- GitHub: https://github.com/weegs710/peepers
