# Building and testing

## The shell

```console
$ nix-shell       # or, with flakes: nix develop
```

[`shell.nix`](../shell.nix), which the flake also exposes as its dev shell, provides `pkg-config`, the libraries peepers draws with (wayland, libxkbcommon and vulkan-loader), `LD_LIBRARY_PATH` pointing at them, and sets `CARGO_TARGET_DIR` to `~/.cache/cargo-target/peepers`, so a build never lands in the checkout.

Outside Nix, install what the [README](../README.md) lists under Requirements, plus `pkg-config`, and make sure the binary can find the libraries at run time.

To run one command in the shell without staying in it:

```console
$ nix-shell --run 'cargo build --release'
```

## Build and run

```console
$ cargo build --release
$ cargo run --release
```

The release profile uses thin LTO, a single codegen unit and strips the binary.

## The Nix package

[`package.nix`](../package.nix) builds peepers with `rustPlatform.buildRustPackage`, and [`flake.nix`](../flake.nix) exposes it as `packages.<system>.default` for every Linux system in nixpkgs' `lib.systems.flakeExposed`:

```console
$ nix build .#default
$ ./result/bin/peepers
```

The package finds wayland, libxkbcommon and the Vulkan loader through the binary's RPATH, so it runs without `nix-shell`, and the commands peepers runs do not inherit an `LD_LIBRARY_PATH`. It also installs the desktop entry and icon under `share/`, the theme templates under `share/peepers/themes/`, and the theme guide under `share/doc/peepers/`.

## Tests

```console
$ cargo test --workspace
$ cargo clippy --workspace --all-targets
$ cargo fmt --all
```

The interface tests draw the real views with `iced_test`'s simulator, click what a user would click, and apply the messages that come back. They open no window. `crates/core` also has integration tests under `tests/` for the disk benchmark and the flight recording files.

### Tests that need a session bus

Tests that need a session bus are marked `#[ignore]` with that reason, so a machine without one still passes. Run the command door's with:

```console
$ cargo test -p peepers-core ipc -- --ignored --test-threads=1
```

Each test claims a bus name of its own, so a peepers already running on the same bus cannot answer it. Run them one at a time (`--test-threads=1`), because the names belong to the process.

### Screenshots

One ignored test draws every page to a PNG for visual review:

```console
$ mkdir shots
$ PEEPERS_SHOTS=shots cargo test -p peepers render_pages -- --ignored
```

Use an empty directory. When an image is already there the test compares against it and fails if the page has changed.
