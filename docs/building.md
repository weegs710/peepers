# Building and testing

## The shell

```console
$ nix-shell
```

[`shell.nix`](../shell.nix) provides `pkg-config`, the libraries peepers draws with (wayland, libxkbcommon and vulkan-loader), `LD_LIBRARY_PATH` pointing at them, and sets `CARGO_TARGET_DIR` to `~/.cache/cargo-target/peepers`, so a build never lands in the checkout.

Outside Nix, install a Rust toolchain that supports edition 2024, `pkg-config`, and those three libraries, and make sure the binary can find them at run time.

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
