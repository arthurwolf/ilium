### Fixed

- Fixed the Ubuntu VM sandbox qualification failing because the guest's user manager delegated only the memory and pids controllers; the VM preparation now delegates the cpu controller too.
