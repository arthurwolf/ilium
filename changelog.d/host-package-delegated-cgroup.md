### Fixed

- The Linux host package qualification (deb, AppImage, Snap, Flatpak) now runs the installed animation probe in a delegated systemd service and gives Flatpak a session bus (the script re-executes itself under dbus-run-session), so the probe finds the cgroup it needs on a CI runner.
- The host delegated service names the cpu, memory and pids controllers explicitly so a runner's parent slice enables them.
- The same service sets memory, task and CPU limits, which makes systemd enable those controllers in its parent slice.
- The probe service moves into a child cgroup before enabling controllers, which older systemd (Ubuntu 22.04) requires.
