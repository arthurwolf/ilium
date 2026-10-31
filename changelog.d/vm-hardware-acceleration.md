### Fixed

- The Linux release workflows now run the Ubuntu 22.04 package VM with KVM hardware acceleration when the runner exposes it, because software emulation was too slow for the animation helper deadline; they fall back to software emulation otherwise.
