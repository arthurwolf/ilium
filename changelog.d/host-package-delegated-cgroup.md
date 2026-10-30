### Fixed

- The Linux host package qualification (deb, AppImage, Snap, Flatpak) now runs the installed animation probe in a delegated systemd service and gives Flatpak a session bus, so the probe finds the cgroup it needs on a CI runner.
