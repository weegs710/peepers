# Platforms

What peepers reads and what it needs from the system around it.

peepers is developed and tested on NixOS under the Umbriel compositor. Other distributions and compositors are not tested. The few places that know about NixOS are listed under [NixOS paths](#nixos-paths).

## What it reads

| Reads                                                    | From                                        |
| -------------------------------------------------------- | ------------------------------------------- |
| processes and their threads                              | `/proc/<pid>`, `/proc/stat`                 |
| user names                                               | `/etc/passwd`, and the sockets in `/run/systemd/userdb` for the accounts systemd makes for services. See [Sensitive reads](#sensitive-reads) |
| uptime and load                                          | `/proc/uptime`, `/proc/loadavg`             |
| processors                                               | `/proc/cpuinfo`, `/sys/devices/system/cpu`, `/sys/devices/system/node` |
| memory and swap                                          | `/proc/meminfo`, `/proc/vmstat`, and `/proc/spl/kstat/zfs/arcstats` where ZFS is loaded |
| pressure stall information                               | `/proc/pressure/cpu`, `memory` and `io`     |
| disks                                                    | `/proc/diskstats`, `/sys/block`             |
| network interfaces                                       | `/proc/net/dev`, `/proc/net/wireless`, `/sys/class/net` |
| sockets                                                  | `/proc/net`                                 |
| graphics adapters                                        | `/sys/class/drm`, and the DRM clients' fdinfo under `/proc` |
| sensors                                                  | `/sys/class/hwmon`                          |
| package power                                            | `/sys/class/powercap`                       |
| energy history and battery discharge                     | powercap, hwmon, `/sys/class/power_supply`, process CPU counters and cgroups; persisted in the XDG state directory |
| PCI device names                                         | the udev database in `/run/udev/data`       |
| mounted volumes                                          | `/proc/self/mountinfo`                      |
| kernel modules                                           | `/proc/modules`, `/sys/module`, and the module metadata under `/lib/modules/<release>` |
| the system report                                        | `/etc/os-release`, `/proc/sys/kernel/hostname` and `osrelease` |
| desktop entries and autostart entries                    | the XDG data and config directories         |
| noctalia's colours, while the Noctalia theme is chosen   | `noctalia.toml` in `$XDG_CONFIG_HOME/peepers` |

peepers runs two external programs: `nvidia-smi`, where an NVIDIA adapter's readings come from, and `sh -c`, which runs the command typed into the Processes page.

## Sensitive reads

A task manager has to look at some files that sound alarming out of context. Here is each one, exactly what peepers takes from it, and why it needs it.

### `/etc/passwd`

- **Taken**: each line's user name and numeric ID. The other fields are skipped and nothing else is kept.
- **Why**: to show a name where the process table would otherwise show a number. The file is world-readable.

### The sockets in `/run/systemd/userdb`

- **Taken**: one request per process owner that `/etc/passwd` does not name, asking for that user ID. Only the name and the ID are read from the reply. It also holds a home directory, a shell and a real name, which are dropped when the reply is parsed.
- **Why**: to name the accounts systemd creates for services (`DynamicUser`), which are not in `/etc/passwd`. The directory is root-owned, so only root can put a service there, and each request waits at most 200 ms.

### `/proc/<pid>/cmdline`

- **Taken**: the command line of the one process selected on the Processes page.
- **Why**: to show what that process is. A command line can carry a secret passed as an argument, and `ps` shows the same.

### `/proc/<pid>/exe`, `cwd`, `fd`, `fdinfo` and `io`

- **Taken**: the executable and working directory of the selected process, which sockets and graphics devices processes hold open, and how much they read and write.
- **Why**: the process panel, the socket owners on Connections, and per-process graphics use. The kernel limits these to your own processes unless peepers runs with privilege.

peepers never opens `/etc/shadow`, and never reads a process's `environ`, `mem` or `maps`.

The only connection peepers opens is the internet benchmark's, to the server list at `librespeed.org` and the servers it names, and only when asked for. Nothing above is sent.

## What it needs a bus for

| Bus     | Name                              | Used for                                                  |
| ------- | --------------------------------- | --------------------------------------------------------- |
| session | `dev.weegs.Peepers`               | the command line and [D-Bus control](./control.md)        |
| session | `org.kde.StatusNotifierWatcher`   | the tray icon, which needs a host to register with        |
| session | `org.freedesktop.systemd1`        | the Services page, for the user's own manager             |
| system  | `org.freedesktop.systemd1`        | the Services page, for the system manager                 |
| system  | `org.freedesktop.login1`          | the Users page                                            |
| system  | `org.freedesktop.UDisks2`         | a drive's health, read without root                       |

Without a session bus there is no tray icon and no command line control. The application still runs.

Services and Users need systemd and logind. No other init system is supported.

## NixOS paths

Three places also know about NixOS locations. They have no effect where those paths do not exist.

- Kernel module metadata is looked for in `/lib/modules/<release>` first and then in `/run/booted-system/kernel-modules/lib/modules/<release>`.
- Installed apps counts `/etc/profiles/per-user` as the user's own applications.
- Disk space leaves out `/run/wrappers`, because it is system plumbing and not storage.

## Compositors and display servers

peepers draws with iced's Wayland and X11 backends. It speaks no compositor protocol of its own and runs no compositor's commands, so nothing in it is tied to one compositor.

Window rules can tell the two windows apart by application id, see [The popout](./features.md#the-popout). A key can drive the running copy with the [command line](./control.md), which works through D-Bus and so the same under any compositor.
