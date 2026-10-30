### Fixed

- The Linux host package qualification (deb, AppImage, Snap, Flatpak) now runs the installed animation probe in a delegated systemd service and gives Flatpak a session bus (the script re-executes itself under dbus-run-session), so the probe finds the cgroup it needs on a CI runner.
- The host delegated service names the cpu, memory and pids controllers explicitly so a runner's parent slice enables them.
