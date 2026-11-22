### Fixed
- Stop listing Kilo Proxy sessions (`claude-proxy` / `claude-brainiac`, started outside Ilium) under Claude in the Agent usage view: sessions found on disk whose transcript shows the proxy's `kilo-search` tools now appear in their own "Kilo Proxy · own backend · started outside Ilium" section with no shared account line, and their names no longer start with the launcher boilerplate.
