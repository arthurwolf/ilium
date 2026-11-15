### Added
- Plugin scenes can reach hosts named in one of their own settings: a `http:setting:<key>` grant lends the scene exactly the hosts of the addresses in that setting (`;`-separated, wildcards ignored, plain http only to loopback), in the render instance and in its jobs.
- The Scenes plugin can download large files in the background: its new `fetch-to-cache` job fetches a file in 4 MiB ranged chunks into the plugin's private cache, resumes after its deadline, checks a pinned length and SHA-256, and removes the oldest cached files once the cache passes 1 GiB.
