# Security policy

## Supported versions

peepers has no numbered releases yet. Only the newest commit on `main` is supported, and fixes land there.

The Nix package follows the same rule. The prebuilt binaries in the `peepers` cache on cachix are builds of `main`, one for each commit that CI built. Builds of older commits stay in the cache but get no fixes, so run `nix flake update peepers` to move to the newest one.

Check that the problem is still there on the newest `main` before you report it.

## Reporting a vulnerability

Do not open an issue, a discussion or a pull request for a security problem, because those are public.

Report it privately instead: open the [Security tab](https://github.com/weegs710/peepers/security), go to **Advisories** and click **Report a vulnerability**. If you cannot use GitHub, email [weegs710@tutamail.com](mailto:weegs710@tutamail.com).

Put in the report:

- what the problem is, and what someone could do with it
- the commit you tested on
- how you installed peepers: cargo, the Nix package, or something else
- the steps to make it happen

## What peepers touches

These are the places a security problem is most likely to be:

- **Processes**: it reads `/proc` for every process it is allowed to see, including command lines, and sends the signals you pick.
- **Run new task**: it runs the command you type through `sh -c`, as you.
- **Installed apps and Startup apps**: it reads the desktop entries in the XDG data and config directories, and turning a startup app on or off writes your own copy of its entry into your `autostart` directory.
- **Services and Users**: starting, stopping and restarting services, and locking or ending sessions, go to systemd and logind on the system bus, and polkit decides whether you are allowed.
- **Drive health**: it asks udisks2 on the system bus.
- **The control interface**: it owns `dev.weegs.Peepers` on the session bus, which any program in your session can call to show or hide its windows, pause, record, open a page or quit it. See [Controlling a running peepers](./docs/control.md).
- **NVIDIA graphics**: it runs `nvidia-smi`.
- **Files it saves**: the settings file ([The settings file](./docs/settings.md)), energy history, flight recordings, benchmark results and the disk benchmark's test file, all as you.
- **Nix**: the flake, the package and the `peepers` cache on cachix.

A problem in systemd, polkit, udisks2, iced or another project peepers uses belongs with that project. If peepers uses one of them in an unsafe way, report that here.
