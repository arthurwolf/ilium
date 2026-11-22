### Fixed
- Kilo Gateway is never refused over paid proxies: with proxies enabled but none loaded (or the proxy database unreadable at startup) it now calls directly instead of failing; with proxies loaded it still routes through them.
