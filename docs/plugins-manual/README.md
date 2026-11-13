# Plugin developer manual: build notes

## Build path used

Fallback path: `build_fallback.py`, run by `build.sh` with `/usr/bin/python3`.

Why:

- Jekyll is not installed. `gem install --user-install jekyll` failed on the first try: the `http_parser.rb` native extension did not compile (`extconf failed, exit code 1`). A second attempt was not made because the failure is in a native build step and the same toolchain would be used again.
- The Python `markdown` package (3.10.2) is present in `/usr/bin/python3`. The `python3` on PATH is a pyenv virtualenv shim where a user install is refused, so the build pins `/usr/bin/python3`.
- `build.sh` still runs `jekyll build` when `jekyll` is on PATH. That branch has not been exercised here.

## Layout

- `_config.yml`, `_layouts/`, `_includes/`, `_data/nav.yml`: Jekyll-shaped source.
- `index.md`, `pages/`: content. Every page has `layout`, `title` and `permalink` front matter.
- `assets/css/manual.css`: the stylesheet (dark default, light via `prefers-color-scheme` or `data-theme`).
- `build.sh`: entry point; prints JSONL (`progress`, `result`, `error`) and exits non-zero on error.
- `build_fallback.py`: the fallback builder. It supports front matter, `{{ }}` variables, `| relative_url`, `{% include %}`, one level of `{% for %}`. Markdown goes through the `markdown` package (tables, fenced code, raw HTML passthrough).
- `_site/`: generated output, rebuilt in full on every run.

## Serving

Systemd user unit `~/.config/systemd/user/ilium-plugins-manual.service` serves `_site/` with
`python3 -m http.server 4327 --bind 127.0.0.1`. Port 4327 was free when the unit was created. The unit runs `/usr/bin/python3` (the same interpreter the build uses). Rebuild with `./build.sh`; the server reads files from disk, so no restart is needed.

URL: `http://127.0.0.1:4327/`

## Limits

- The fallback does not evaluate Liquid inside page bodies, so `{{ }}` in prose or code is literal.
- Liquid features beyond the subset above are not supported by the fallback.
- Front matter and `_data/nav.yml` use flat `key: value` lines only.
