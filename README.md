# peepers

A system monitor and task manager for Linux, written in Rust with [iced](https://iced.rs). One window of pages for watching, recording, inspecting and managing the machine, a [popout](./docs/features.md#the-popout) of live gauges, a tray icon, and a command line that drives the running copy.

> **Requirements**: Linux, a Rust toolchain that supports edition 2024, and three libraries: wayland, libxkbcommon and vulkan-loader. The tray icon and the command line need a session D-Bus, the Services and Users pages need systemd, and drive health needs udisks2 running. See [Platforms](./docs/platforms.md) for everything it reads.

## Quickstart

```console
$ git clone https://github.com/weegs710/peepers.git && cd peepers
$ cargo build --release
$ peepers                         # the window
$ peepers --toggle-pop            # from another terminal: open or close the popout
$ peepers --page services         # show a page, opening the window if it is closed
$ peepers --flight start          # start a flight recording, no window needed
```

`peepers` is `target/release/peepers` until you put it on your `PATH`. See [Install](#install), which also covers NixOS.

## What It Shows

| Section | Pages                                                                |
| ------- | -------------------------------------------------------------------- |
| WATCH   | Processes, Performance, Pressure, Power                              |
| RECORD  | Flight recorder, Benchmarks                                          |
| INSPECT | Connections, Drivers, Disk space, Installed apps, System information |
| MANAGE  | Services, Startup apps, Users                                        |
| PEEPERS | Settings                                                             |

## Documentation

[Features](./docs/features.md) ·
[Themes](./docs/themes.md) ·
[Energy history](./docs/energy.md) ·
[Controlling a running peepers](./docs/control.md) ·
[The settings file](./docs/settings.md) ·
[Architecture](./docs/architecture.md) ·
[Platforms](./docs/platforms.md) ·
[Building and testing](./docs/building.md)

## Themes

peepers comes with the themes that [iced](https://iced.rs) ships, six more of its own (Cold Wind, Cymagentan, Great Below, Plasm, Triglavian and Twilight), and **Noctalia**, which follows the palette of the [noctalia](https://noctalia.dev) shell and changes when it does. Noctalia needs one template added to noctalia first. See [Themes](./docs/themes.md).

## Command line

With no option, `peepers` starts the application. With one, it asks the copy that is already running to act, and exits:

```console
$ peepers --toggle-window         # show the main window, or hide it
$ peepers --pause                 # hold the numbers still
$ peepers --quit save             # end it, saving a running recording first
```

Every option, the exit codes, and the same requests over D-Bus are in [Controlling a running peepers](./docs/control.md).

## Install

Install what the requirements above list, plus `pkg-config`, then:

```console
$ cargo build --release
$ install -Dm755 target/release/peepers ~/.local/bin/peepers
```

**Launcher and icon.** [`packaging/peepers.desktop`](./packaging/peepers.desktop) and [`packaging/peepers.png`](./packaging/peepers.png) give application menus, docks and window lists the name and icon:

```console
$ install -Dm644 packaging/peepers.desktop ~/.local/share/applications/peepers.desktop
$ install -Dm644 packaging/peepers.png ~/.local/share/icons/hicolor/512x512/apps/peepers.png
```

The entry runs `peepers` from your `PATH`.

Start it once per session, for example from your compositor's autostart.

### NixOS

The repository is a flake. Try it without installing anything:

```console
$ nix run github:weegs710/peepers
```

To install it, add the flake as an input and the package to your system. It brings the desktop entry and the icon with it. Prebuilt binaries are in the `peepers` cache on cachix, so the `nix.settings` lines let you download peepers instead of compiling it:

```nix
{
  inputs.peepers.url = "github:weegs710/peepers";

  outputs = { nixpkgs, peepers, ... }: {
    nixosConfigurations.yourhost = nixpkgs.lib.nixosSystem {
      modules = [
        {
          environment.systemPackages = [ peepers.packages.x86_64-linux.default ];
          services.udisks2.enable = true; # drive health
          nix.settings = {
            substituters = [ "https://peepers.cachix.org" ];
            trusted-public-keys = [ "peepers.cachix.org-1:rP90IXXdICg4J7eVWuddscdW8QCobU/a945JKWcxWF8=" ];
          };
        }
      ];
    };
  };
}
```

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md).

## License

MIT, please see [LICENSE](LICENSE).

## Links

- GitHub: https://github.com/weegs710/peepers
