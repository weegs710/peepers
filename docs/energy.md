# Energy history

Energy history is the last section of **Power**, below the live sensors. It starts folded each time peepers starts; click its heading to open or fold it. A storage problem is shown under the heading even while it is folded. Collection starts automatically with peepers and continues while its windows are hidden or its display is paused. peepers must remain running; exiting it stops collection. The existing tray and desktop autostart facilities can keep it running during a login session.

Choose **24 hours**, **7 days**, **30 days**, or **90 days**, filter the rankings, then select a name to see its timeline. The day view has hourly bins; longer views show UTC days. Ranges include the current, incomplete hour and the preceding hours, so the displayed start and end timestamps are the precise bounds. The first and last day in a longer view can be partial.

For apps, services and device activity, switch between **Energy timeline** and **Activity timeline**. Activity remains useful when no energy readings are available.

## What the numbers mean

| Reading | Meaning |
| --- | --- |
| Energy, Wh | Accumulated energy during intervals with available readings |
| Average, W | Energy divided by the duration with available readings, not by the whole selected date range |
| Peak, W | Highest sampled interval average; very brief peaks can be missed |
| Active | Time in sampled intervals with CPU activity for apps/services, or I/O/GPU activity for device activity rows |
| Bursts | Transitions from an inactive or absent sampled group to an active one; these are not scheduler wakeups |
| Available / observed | Time with energy readings versus time the item was observed |
| — | Unavailable, not zero |

Hardware channels are **separate and may overlap**. A CPU package includes its cores; a platform counter may include the CPU; battery discharge covers the laptop. Never add all hardware rows to calculate a machine total. The CPU packages row combines the non-overlapping top-level package domains only. It is unavailable if any package lacks a consecutive energy reading. The platform (`psys`) counter is shown separately, with no assumption that its scope equals wall-socket consumption.

RAPL energy comes from counter deltas, with wrap handling. Power sensors, including GPU sensors already supported by peepers, are integrated using the average of consecutive readings. Battery current times voltage is used when battery power is not exposed directly. Only **discharging** battery intervals are counted: charging current is not the machine's consumption. Hardware or drivers without power telemetry cannot supply watts; supported disk, physical network, and GPU activity is still recorded in separate activity-only rows.

App and service numbers are explicitly **CPU estimates**, using:

```text
group energy = CPU package energy × group CPU usage / total logical CPU capacity
```

If staggered process reads make summed CPU usage exceed capacity, the shares are normalized. Idle capacity, kernel work not attributed to a sampled process, and shared consumption remain in **Idle / shared / unattributed CPU**. Group estimates plus that remainder equal the measured package energy for each interval. This is a simple allocation model, not a calibrated model of heterogeneous cores, instructions, or idle power. It does not attribute GPU, disk, network, display, or peripheral energy to apps, and it does not predict the energy saved by stopping an app.

Processes in the same systemd application scope or service are grouped, with application launch-instance suffixes (including a PID followed by a launch counter) removed. Otherwise they are grouped by executable name; inaccessible executable paths fall back to process names. Matching application names share one entry across PIDs, UIDs, restarts and executable paths, including different installed builds. Concurrent instances are combined before accounting, so their energy adds up while each sampled interval counts once toward coverage and activity. Service groups retain their service boundary and owning UID. Group identities are refreshed every 30 seconds and when the process identity or name changes. PID reuse does not inherit old CPU/I/O counters. Short-lived processes between samples, inaccessible processes, migrations between cgroups, and launches without distinct app scopes limit attribution.

Existing history uses the same application grouping in both rankings and timelines without rewriting saved files. Energy from older split entries is added exactly. When several entries merge within an hour, coverage and activity are summed and capped at the recorded duration; averages, peaks and bursts are also approximate because the original hourly aggregates lack overlap timing. The report shows a notice when this applies. New samples record combined group activity and power directly.

## Persistence and gaps

History is stored in `$XDG_STATE_HOME/peepers/energy`, falling back to `~/.local/state/peepers/energy`. It uses one versioned JSON file per UTC hour and retains 90 days. Files contain group names, executable/cgroup identities, UIDs, timestamps and aggregates, not command lines. New directories are private and new files have mode `0600`.

The sampler collects before handing readings to the UI, so a busy, paused, or hidden interface cannot drop accounting samples. Changing the sampling interval preserves the collector and its counter baselines. History refreshes on screen every ten seconds; changing the range requests a new report at the next sample.

Files are checkpointed every 30 seconds and when quitting normally. A crash or forced termination can lose the uncheckpointed interval. Writes use an atomic rename, and a lock prevents a second peepers instance from overwriting the history. A storage failure is shown on Power; collection can continue in memory. If saving on Quit fails, peepers offers a retry or an explicit exit without saving. Corrupt or unsupported files are reported and preserved, including when this prevents saving new readings for the same hour. Hourly files larger than 16 MiB are rejected without replacing their previous checkpoint.

There is no interpolation across startup, failed samples, suspend/resume, clock jumps, or sampling delays longer than 30 seconds. A missing sensor starts a new baseline when it returns. Empty timeline bins remain gaps. Partial bins show coverage; their energy is not extrapolated to a full hour or day. Overlapping timestamps after a restart or clock adjustment are not counted twice.

## Reading CPU counters as a normal user

Linux commonly restricts `/sys/class/powercap/intel-rapl:*/energy_uj` to root. peepers reports this explicitly and continues recording activity. It does not run the GUI as root or change system permissions itself.

An administrator can give a dedicated group read access to **only `energy_uj`**, leaving power-limit controls untouched. For example, this optional NixOS configuration grants access to the named user at boot:

```nix
{ pkgs, ... }: {
  users.groups.peepers-energy = {};
  users.users.YOUR_USER.extraGroups = [ "peepers-energy" ];
  systemd.services.peepers-energy-permissions = {
    description = "Allow peepers users to read RAPL energy counters";
    wantedBy = [ "multi-user.target" ];
    after = [ "systemd-modules-load.service" ];
    serviceConfig.Type = "oneshot";
    script = ''
      for counter in /sys/class/powercap/intel-rapl:*/energy_uj; do
        [ -f "$counter" ] || continue
        ${pkgs.coreutils}/bin/chgrp peepers-energy "$counter"
        ${pkgs.coreutils}/bin/chmod g+r "$counter"
      done
    '';
  };
}
```

Replace `YOUR_USER`, apply your NixOS configuration, and start a fresh login session for group membership to take effect. Restart the permission service if a powercap driver is loaded or reloaded later. peepers starts showing energy after two readable samples. Access is deliberately restricted to the dedicated group because high-resolution energy counters can expose information about other workloads.

This configuration is an optional deployment example; building peepers does not install it.
