# Features

What each page shows and what it can change, in the order the navigation rail lists them. Each page has an icon beside its name in the rail.

Every table sorts by its columns. Click a heading to sort by that column, and an arrow beside it shows the direction. Click it again to reverse. Number columns start with the largest value first and text columns start at A. The tables are Processes, Connections, Drivers, Disk space (folders, files and file types), Installed apps, Startup apps, Services, Users and the Benchmarks history.

In Processes, Connections, Drivers, Installed apps, Services and Users, the row under the pointer is shaded and the selected row is marked across its whole width. Press anywhere on a row to select it.

A line that reports what an action did goes away by itself after a few seconds when the action worked. A failure, and a note such as the Benchmarks one about older results, stays until you press Dismiss.

## Watch

### Processes

A table of every running process: name, PID, user, CPU, memory and thread count. Turn on Process tree to show each process under its parent, and collapse a branch to hide its children. Filter by name, user or PID.

Selecting a process opens a panel for it:

- **Actions**: End task, Force quit, End process tree, Force quit tree, Suspend and Resume.
- **Processor affinity**: one checkbox per logical processor. A change applies immediately.
- **Priority**: a drop-down.
- **Sample**: watches the process for a few seconds and saves a report of where its threads spent their time, how much memory it held and what it kept open.

A box at the top runs a command you type.

### Performance

One card for each device: the processor, memory, each disk, each graphics adapter and each network interface. A disk's card shows its health as last read, or why it could not be read. The cards rearrange to fit the width of the window.

### Pressure

How long tasks spend stalled waiting for the processor, for memory and for I/O, from the kernel's pressure stall information. Each resource has a one-sentence summary of what the machine is waiting for, and a list of the heaviest processes. In the I/O list, processes blocked on the disk come first and then the busiest, because blocked processes are what a storage stall is made of. Select a process to see its details below the lists.

Per-process read and write figures are only available for your own processes.

### Power

Package power, each logical processor's clock, and the machine's sensors, with graphs. Sensors are named by what they measure, such as the processor or a memory module, because the name the hardware reports for those is that of a chipset bridge. The kernel usually restricts power counters to privileged users, so package power can be unavailable when peepers runs as a normal user.

## Record

### Flight recorder

Records the machine's vitals over time. Start and stop a recording on the page or with [`--flight`](./control.md), give it a name, and choose the folder it is saved to.

While a recording runs, the page and the navigation rail show how long it has run and how long it has left. A recording stops at 172,800 samples or when its file would reach 256 MiB, whichever comes first, so any recording that was made can be opened again. The time left comes from the nearer of the two limits at the pace samples are arriving.

The sample limit is 48 hours at the default one-second interval, and the time scales with the interval set on the Settings page:

| Interval | Longest recording |
| -------- | ----------------- |
| 100 ms   | 4.8 hours         |
| 250 ms   | 12 hours          |
| 500 ms   | 24 hours          |
| 1 s      | 48 hours          |
| 2 s      | 4 days            |
| 5 s      | 10 days           |

When a recording is full, peepers says so and keeps what was captured until you stop and save it.

Saved recordings are listed. Replay one as graphs, with play and pause and a cursor at the replay time, or delete it. Deleting one recording, or all of them, asks first.

### Benchmarks

Processor, graphics and disk tests, run one after another from one button, and an internet test that contacts a server and so runs only when you ask for it. Each result goes into a history with a plain-language note on what the figure means, and the history can be cleared. A run can be stopped, except the graphics test, which cannot be interrupted once it has started.

## Inspect

### Connections

The machine's TCP, UDP and UNIX sockets, with the process that owns each, its local and remote endpoints, its state and its exposure. Filter by process, port or address, and by scope: all connections, listening ports, or ports open to every interface or the internet. UNIX sockets stay hidden until you tick UNIX sockets.

The Exposure column says how far a listening socket reaches: this computer only, the local network, all interfaces or a public address. An outbound connection has none. UDP and UNIX sockets have no handshake, so they show only the states that apply to them, such as Bound or Connected. Select a row to see its details.

### Drivers

The loaded kernel modules, with their size, state, how many modules use them and their source version. Filter by name. Built-in modules stay hidden until you tick Built-in modules. Select a module to see its path, what uses it, what it depends on, its parameters and any taint.

### Disk space

Shows each mounted volume and how full it is. To find what is using the space, pick a volume or type a folder and press Scan. peepers measures it and lists the largest folders, the largest files and a breakdown by file type.

A scan can be cut short with Stop scanning. The results then cover what was measured up to that point, and the summary says the scan was stopped early.

### Installed apps

The desktop entries on the system, filtered by name or category. The checkbox "Show entries without a launcher" adds the entries marked `NoDisplay`, which exist for file-type handlers and helpers. Refresh reads them again.

### System information

A report on this machine in six sections: System, Processor, Memory, Storage, Network and Graphics. Export saves it to a folder.

## Manage

### Services

systemd services from the system manager and from your own user manager, with each service's state in plain words (Running, Stopped, Failed and so on). Filter by name or description, or show only active services. Select a service to see its load state and detail, and to start, stop or restart it. Changing a system service goes through polkit, so you may be asked to authenticate.

### Startup apps

The autostart entries, with who each is defined for (this user or everyone) and its command. The button on each row switches an entry on or off.

### Users

Every user that owns a running process, with their sessions, process count, CPU and memory. Select a user to lock or sign off their session, when they have one.

## Peepers

### Settings

| Setting                | Choices                                                   |
| ---------------------- | --------------------------------------------------------- |
| Theme                  | any theme iced lists                                      |
| Start on               | any page                                                  |
| Temperatures           | Celsius or Fahrenheit                                     |
| Read the machine every | 100 ms, 250 ms, 500 ms, 1 s, 2 s or 5 s                  |
| Text size              | 12 to 24 in steps of 2, also `Ctrl` with `+`, `-` and `0` |
| Tray icon              | on or off                                                 |

Each change is saved as it is made. The page shows where the settings file is, and [The settings file](./settings.md) describes it.

## The popout

A narrow second window of live gauges: the same device cards as Performance, in a single column. Open it from the rail, from the tray menu or with `--toggle-pop`. It stays open when the main window is closed, and its graphs keep updating while it is the only window.

## The tray icon

An icon that shows the processor and memory load, when a tray host is running. It has a panel for each, marked C and M, with the last 13 readings as columns that rise from the bottom, the newest on the right, and go from green to amber to red as the load climbs. A reading is added for every sample, so the trace spans 13 sample intervals. The tooltip gives the latest percentages. Its menu has Window, Popout, Pause and Quit. Activating the icon brings the window forward, or hides it when it is already in front. Closing the main window hides it to the tray while the icon exists. Without the icon, closing the main window ends the application.
