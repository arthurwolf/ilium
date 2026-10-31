### Fixed

- The Linux host package qualification (deb, AppImage, Snap, Flatpak) now runs the installed animation probe in a delegated systemd service and gives Flatpak a session bus (the script re-executes itself under dbus-run-session), so the probe finds the cgroup it needs on a CI runner.
- The host delegated service names the cpu, memory and pids controllers explicitly so a runner's parent slice enables them.
- The same service sets memory, task and CPU limits, which makes systemd enable those controllers in its parent slice.
- The probe service moves into a child cgroup before enabling controllers, which older systemd (Ubuntu 22.04) requires.
- Fixed the AppImage host check failing on the byte-check lines its wrapper wrote to stderr, and the Flatpak host check running its server outside the delegated cgroup.
- Fixed the Flatpak host check failing because its throwaway D-Bus session could not mount the document portal; the check now runs `flatpak run` with `--no-documents-portal`.
