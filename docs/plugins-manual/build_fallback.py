#!/usr/bin/env python3
"""Fallback static builder for the plugin developer manual, used when Jekyll is not installed.

Supports the Jekyll subset the manual uses:
- front matter (flat key: value lines) on pages and layouts
- {{ page.x }}, {{ site.x }}, {{ content }} and 'literal' | relative_url
- {% include file %} and one level of {% for item in list %} ... {% endfor %}
- _config.yml (flat key: value) and _data/nav.yml (list of flat mappings)

Markdown is converted with the Python markdown package (tables, fenced code,
raw HTML passthrough). Page bodies are not run through the Liquid subset, so
braces inside prose and code blocks are safe.

Output: one JSON object per line on stdout, with a "type" field
(progress, result or error). Exit status 0 on success, 1 on error.
"""

import html
import json
import re
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SITE_DIR = ROOT / "_site"
STAGING_DIR = ROOT / "_site.staging"
LAYOUT_DIR = ROOT / "_layouts"
INCLUDE_DIR = ROOT / "_includes"
EXCLUDED_TOP_LEVEL_FILES = {"README.md", "CONVENTIONS.md"}
REQUIRED_FRONT_MATTER_KEYS = ("layout", "title", "permalink")
MARKDOWN_EXTENSIONS = ["tables", "fenced_code", "toc", "attr_list", "sane_lists"]

FRONT_MATTER_PATTERN = re.compile(r"\A---\n(.*?)---\n", re.DOTALL)
FOR_BLOCK_PATTERN = re.compile(r"\{%\s*for\s+(\w+)\s+in\s+([\w.]+)\s*%\}(.*?)\{%\s*endfor\s*%\}", re.DOTALL)
INCLUDE_PATTERN = re.compile(r"\{%\s*include\s+([\w./-]+)\s*%\}")
VARIABLE_PATTERN = re.compile(r"\{\{\s*(.*?)\s*\}\}", re.DOTALL)


RAW_VARIABLES = {"content", "sidebar", "rail", "pager", "crumbs"}

# Sidebar groups, in display order. A page belongs to the first group whose predicate matches its slug.
GUIDE_SLUGS = {"settings-screens", "porting-iliumanim", "rust-sdk", "typescript-sdk", "performance",
               "accessibility-and-theming"}
PROJECT_SLUGS = {"glossary", "security", "roadmap", "design-decisions"}
START_SLUGS = {"landing"}
CONCEPT_SLUGS = {"architecture", "worlds-and-sides", "retained-mode", "events-and-intents", "requests-and-outcomes",
                 "lifecycle", "capabilities", "packages", "ui-model", "layout", "primitives", "file-access",
                 "background-jobs", "settings-extension", "services", "extending-core-plugins", "errors-and-limits", "versioning", "testing", "debugging"}
NAV_GROUPS = [
    ("Start here", lambda s: s in START_SLUGS or s.startswith("getting-started-")),
    ("Core plugins", lambda s: s == "core-plugins" or s.startswith("core-plugin-")),
    ("Tutorials", lambda s: s.startswith("tutorial-")),
    ("Concepts", lambda s: s in CONCEPT_SLUGS),
    ("Host API", lambda s: s.startswith("api-")),
    ("Pane primitives", lambda s: s.startswith("primitives-")),
    ("Widgets", lambda s: s.startswith("widgets-")),
    ("Capabilities", lambda s: s.startswith("capability-")),
    ("Agent docs", lambda s: s.startswith("agent-docs")),
    ("Reference", lambda s: s == "reference" or s.startswith("reference-")),
    ("Guides", lambda s: s in GUIDE_SLUGS or s.startswith("guide-")),
    ("For users", lambda s: s.startswith("user-")),
    ("Project", lambda s: s in PROJECT_SLUGS),
]
SVG_OPEN_PATTERN = re.compile(r"<svg\b([^>]*)>")
VIEWBOX_PATTERN = re.compile(r'viewBox="\s*[\d.]+\s+[\d.]+\s+([\d.]+)\s+([\d.]+)\s*"')
SVG_SCALE = 1.5
HEADING_PATTERN = re.compile(r'<h2 id="[^"]*">(.*?)</h2>', re.DOTALL)


class BuildError(Exception):
    """A problem in the manual sources that stops the build."""


def emit(record_type, **fields):
    print(json.dumps({"type": record_type, **fields}, ensure_ascii=False), flush=True)


def parse_scalar(raw_value):
    value = raw_value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
        return value[1:-1]
    return value


def parse_flat_yaml(text):
    data = {}
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("#") or line[0] in " \t":
            continue
        key, separator, raw_value = stripped.partition(":")
        if separator:
            data[key.strip()] = parse_scalar(raw_value)
    return data


def parse_nav_yaml(text):
    items = []
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        if stripped.startswith("- "):
            items.append({})
            stripped = stripped[2:].strip()
        if not items:
            raise BuildError("_data/nav.yml: a key appears before the first '- ' item")
        key, separator, raw_value = stripped.partition(":")
        if not separator:
            raise BuildError(f"_data/nav.yml: cannot parse line {line!r}")
        items[-1][key.strip()] = parse_scalar(raw_value)
    for item in items:
        if "title" not in item or "url" not in item:
            raise BuildError(f"_data/nav.yml: every item needs title and url, got {item!r}")
    return items


def slug_of(url):
    return url.rstrip("/").rsplit("/", 1)[-1]


def group_of(url):
    slug = slug_of(url)
    for group_name, matches in NAV_GROUPS:
        if matches(slug):
            return group_name
    return None


def ordered_nav(nav):
    """Nav items in sidebar order: group order first, then the order of nav.yml inside a group."""
    grouped = {name: [] for name, _ in NAV_GROUPS}
    for item in nav:
        group_name = group_of(item["url"])
        if group_name:
            grouped[group_name].append(item)
    for name in grouped:
        grouped[name].sort(key=lambda item: (slug_of(item["url"]) != "landing", item["url"]) if name == "Start here"
                           else (0, ""))
    return grouped


def build_sidebar(nav, current_url, baseurl):
    grouped = ordered_nav(nav)
    parts = ['<a class="brand" href="%s/"><b>ilium<i>/</i>plugins</b><small>Developer manual</small></a>' % baseurl,
             '<input class="filter" type="search" placeholder="filter pages" aria-label="Filter pages">',
             '<div class="tree">']
    for group_name, items in grouped.items():
        if not items:
            continue
        is_current = any(item["url"] == current_url for item in items)
        parts.append('<details%s><summary>%s<span>%d</span></summary><ul>' %
                     (" open" if is_current else "", html.escape(group_name), len(items)))
        for item in items:
            marker = ' aria-current="page"' if item["url"] == current_url else ""
            parts.append('<li><a href="%s%s"%s>%s</a></li>' %
                         (baseurl, item["url"], marker, html.escape(re.sub(r"^Capability: ", "", item["title"]))))
        parts.append("</ul></details>")
    parts.append("</div>")
    return "\n".join(parts)


def build_pager(nav, current_url, baseurl):
    flat = [item for items in ordered_nav(nav).values() for item in items]
    positions = [index for index, item in enumerate(flat) if item["url"] == current_url]
    if not positions:
        return ""
    index = positions[0]
    cells = []
    if index > 0:
        cells.append('<a class="prev" href="%s%s"><small>Previous</small>%s</a>' %
                     (baseurl, flat[index - 1]["url"], html.escape(flat[index - 1]["title"])))
    if index + 1 < len(flat):
        cells.append('<a class="next" href="%s%s"><small>Next</small>%s</a>' %
                     (baseurl, flat[index + 1]["url"], html.escape(flat[index + 1]["title"])))
    return '<nav class="pager" aria-label="Page navigation">%s</nav>' % "".join(cells)


def build_rail(toc_tokens, title):
    """Entries for the on-this-page rail. A top heading that repeats the title is dropped and its children promoted."""
    def flatten(tokens, level):
        for token in tokens:
            if token["name"].strip().lower() == title.strip().lower():
                yield from flatten(token.get("children", []), level)
                continue
            yield level, token
            yield from flatten(token.get("children", []), 3)

    entries = ['<li class="l%d"><a href="#%s">%s</a></li>' % (level, token["id"], html.escape(html.unescape(re.sub(r"<[^>]+>", "", token["name"]))))
               for level, token in flatten(toc_tokens, 2)]
    if len(entries) < 2:
        return ""
    return '<aside class="rail" aria-label="On this page"><b>On this page</b><ul>%s</ul></aside>' % "".join(entries)


def size_svgs(content):
    """Give every inline svg a width from its viewBox so text stays readable instead of scaling to the column."""
    def fix(match):
        attributes = match.group(1)
        if " width=" in " " + attributes:
            return match.group(0)
        box = VIEWBOX_PATTERN.search(attributes)
        if not box:
            return match.group(0)
        width, height = (float(value) * SVG_SCALE for value in box.groups())
        return '<svg%s width="%d" height="%d">' % (attributes, round(width), round(height))
    return SVG_OPEN_PATTERN.sub(fix, content)


def drop_repeated_title(content, title):
    """Pages repeat their title as the first h2; the layout already shows it as the h1."""
    first = HEADING_PATTERN.search(content)
    if first and html.unescape(re.sub(r"<[^>]+>", "", first.group(1))).strip().lower() == title.strip().lower():
        return content[:first.start()] + content[first.end():]
    return content


def split_front_matter(text):
    match = FRONT_MATTER_PATTERN.match(text)
    if not match:
        return {}, text
    return parse_flat_yaml(match.group(1)), text[match.end():]


def lookup(context, dotted_path):
    current = context
    for part in dotted_path.split("."):
        if not isinstance(current, dict) or part not in current:
            return None
        current = current[part]
    return current


def render_template(template, context):
    def expand_loop(match):
        item_name, list_path, body = match.groups()
        items = lookup(context, list_path) or []
        return "".join(render_template(body, {**context, item_name: item}) for item in items)

    def expand_include(match):
        include_file = INCLUDE_DIR / match.group(1)
        if not include_file.is_file():
            raise BuildError(f"missing include _includes/{match.group(1)}")
        return render_template(include_file.read_text(encoding="utf-8"), context)

    def expand_variable(match):
        expression, *filters = [part.strip() for part in match.group(1).split("|")]
        if expression[:1] in ("'", '"'):
            value = parse_scalar(expression)
        else:
            value = lookup(context, expression)
        for filter_name in filters:
            if filter_name != "relative_url":
                raise BuildError(f"unsupported filter {filter_name!r} in {match.group(0)!r}")
            value = str(context["site"].get("baseurl", "")) + str(value or "")
        if expression in RAW_VARIABLES:
            return str(value or "")
        return html.escape("" if value is None else str(value))

    text = FOR_BLOCK_PATTERN.sub(expand_loop, template)
    text = INCLUDE_PATTERN.sub(expand_include, text)
    return VARIABLE_PATTERN.sub(expand_variable, text)


WIDE_GLYPH = re.compile("([\U0001F000-\U0001FAFF\u2600-\u27BF\u2B00-\u2BFF]\ufe0f?)")
TERM_BLOCK = re.compile(r'(<pre class="term">)(.*?)(</pre>)', re.S)


def pin_wide_glyphs(content):
    """Emoji are two terminal cells wide; pin each to 2ch so render columns stay aligned."""
    def pin_block(match):
        inner = re.sub(r"(<[^>]*>)|" + WIDE_GLYPH.pattern,
                       lambda m: m.group(1) or '<span class="w2">%s</span>' % m.group(2), match.group(2))
        return match.group(1) + inner + match.group(3)
    return TERM_BLOCK.sub(pin_block, content)


def render_page(source_path, site):
    front_matter, body = split_front_matter(source_path.read_text(encoding="utf-8"))
    relative_name = source_path.relative_to(ROOT).as_posix()
    for key in REQUIRED_FRONT_MATTER_KEYS:
        if not front_matter.get(key):
            raise BuildError(f"{relative_name}: front matter is missing '{key}'")

    import markdown

    renderer = markdown.Markdown(extensions=MARKDOWN_EXTENSIONS)
    content = renderer.convert(body)
    content = pin_wide_glyphs(drop_repeated_title(size_svgs(content), front_matter["title"]))
    baseurl = str(site.get("baseurl", ""))
    nav = site["data"]["nav"]
    current_url = front_matter["permalink"]
    page = {**front_matter, "url": current_url}
    chrome = {
        "sidebar": build_sidebar(nav, current_url, baseurl),
        "rail": build_rail(renderer.toc_tokens, front_matter["title"]),
        "pager": build_pager(nav, current_url, baseurl),
        "crumbs": '<p class="crumbs"><b>%s</b></p>' % html.escape(group_of(current_url) or "Manual"),
    }
    layout_name = front_matter["layout"]
    while layout_name:
        layout_file = LAYOUT_DIR / f"{layout_name}.html"
        if not layout_file.is_file():
            raise BuildError(f"{relative_name}: missing layout _layouts/{layout_name}.html")
        layout_front_matter, layout_body = split_front_matter(layout_file.read_text(encoding="utf-8"))
        content = render_template(layout_body, {"site": site, "page": page, "content": content, **chrome})
        layout_name = layout_front_matter.get("layout")
    return content


def output_file_for(permalink):
    relative = permalink.strip("/")
    if permalink.endswith(".html"):
        return STAGING_DIR / relative
    return STAGING_DIR / relative / "index.html"


def source_pages():
    for path in sorted(ROOT.rglob("*.md")):
        parts = path.relative_to(ROOT).parts
        if any(part.startswith(("_", ".")) for part in parts):
            continue
        if len(parts) == 1 and parts[0] in EXCLUDED_TOP_LEVEL_FILES:
            continue
        yield path


def write_staging(site):
    pages = list(source_pages())
    if not pages:
        raise BuildError("no source pages found")
    written_targets = set()
    page_records = []
    for index, source_path in enumerate(pages, start=1):
        relative_name = source_path.relative_to(ROOT).as_posix()
        emit("progress", step="render_page", message=f"Rendering page {index} of {len(pages)}: {relative_name}")
        front_matter, body_text = split_front_matter(source_path.read_text(encoding="utf-8"))
        permalink = front_matter.get("permalink", "")
        markdown_url = permalink.rstrip("/") + ".md" if permalink.endswith("/") and permalink != "/" else None
        if markdown_url:
            page_records.append({"url": permalink, "markdown_url": markdown_url, "body": body_text,
                                 "title": front_matter.get("title", ""),
                                 "description": page_description(body_text)})
            markdown_target = STAGING_DIR / markdown_url.lstrip("/")
            markdown_target.parent.mkdir(parents=True, exist_ok=True)
            markdown_target.write_text(f"# {front_matter.get('title', '')}\n\n{body_text.strip()}\n", encoding="utf-8")
        target = output_file_for(front_matter.get("permalink", ""))
        if target in written_targets:
            raise BuildError(f"{relative_name}: permalink {front_matter.get('permalink')!r} is used twice")
        written_targets.add(target)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(render_page(source_path, site), encoding="utf-8")
    assets_dir = ROOT / "assets"
    if assets_dir.is_dir():
        shutil.copytree(assets_dir, STAGING_DIR / "assets")
    write_llms_files(site, page_records)
    return len(pages)


LLMS_SUMMARY = ("Developer manual for Ilium plugins: WebAssembly components (wasm32-wasip2) written in Rust or "
                "TypeScript that add tree decorations, commands, settings, panels, pane kinds, widgets and ambient "
                "scenes to the Ilium terminal multiplexer. The contract is the WIT package plus the manifest.")
AGENT_KIT_DIR = ROOT / "_agent-kit"


def page_description(body):
    """First prose paragraph of a page body, one line, at most 220 characters."""
    in_fence = False
    paragraph = []
    for line in body.splitlines():
        if line.strip().startswith("```"):
            in_fence = not in_fence
            continue
        text = line.strip()
        skip = in_fence or text.startswith(("#", "|", "<", "!", "{%", "- ", "* ", ">", ":::"))
        if not text or skip:
            if paragraph:
                break
            continue
        paragraph.append(text)
    flat = re.sub(r"\[([^\]]+)\]\([^)]*\)", r"\1", " ".join(paragraph))
    flat = re.sub(r"[`*_]", "", flat)
    return flat if len(flat) <= 220 else flat[:217].rsplit(" ", 1)[0] + "..."


def write_llms_files(site, page_records):
    """llms.txt (llmstxt.org index), llms-full.txt (every page), a .md twin of every page and the agent kit."""
    base = site["url"].rstrip("/") + site["baseurl"]
    by_url = {record["url"]: record for record in page_records}
    lines = [f"# {site['title']}", "", f"> {LLMS_SUMMARY}", "",
             "Every page below is also served as plain Markdown at the same address with `.md` instead of the "
             "trailing slash. `llms-full.txt` holds the whole manual in one file. Ready-made agent instruction "
             "files for plugin projects are under `agent-kit/`.", ""]
    grouped = ordered_nav(site["data"]["nav"])
    for group_name, items in grouped.items():
        entries = [by_url[item["url"]] for item in items if item["url"] in by_url]
        if not entries:
            continue
        lines.append(f"## {group_name}")
        lines.append("")
        for record in entries:
            lines.append(f"- [{record['title']}]({base}{record['markdown_url']}): {record['description']}")
        lines.append("")
    (STAGING_DIR / "llms.txt").write_text("\n".join(lines), encoding="utf-8")

    full = [f"# {site['title']}", "", f"> {LLMS_SUMMARY}", ""]
    for group_name, items in grouped.items():
        for item in items:
            record = by_url.get(item["url"])
            if record:
                full += [f"<!-- source: {base}{record['markdown_url']} -->", f"# {record['title']}", "",
                         record["body"].strip(), "", "---", ""]
    (STAGING_DIR / "llms-full.txt").write_text("\n".join(full), encoding="utf-8")

    if AGENT_KIT_DIR.is_dir():
        shutil.copytree(AGENT_KIT_DIR, STAGING_DIR / "agent-kit")
    return len(page_records)


def main():
    try:
        import markdown  # noqa: F401
    except ImportError:
        emit("error", step="check_markdown",
             message="Python markdown package is not importable; run with /usr/bin/python3 or install markdown")
        return 1

    try:
        emit("progress", step="read_config", message="Reading _config.yml and _data/nav.yml")
        config = parse_flat_yaml((ROOT / "_config.yml").read_text(encoding="utf-8"))
        nav = parse_nav_yaml((ROOT / "_data" / "nav.yml").read_text(encoding="utf-8"))
        site = {
            "title": config.get("title", ""),
            "baseurl": config.get("baseurl", ""),
            "url": config.get("url", ""),
            "data": {"nav": nav},
        }

        if STAGING_DIR.exists():
            shutil.rmtree(STAGING_DIR)
        STAGING_DIR.mkdir()
        try:
            page_count = write_staging(site)
        except Exception:
            shutil.rmtree(STAGING_DIR, ignore_errors=True)
            raise

        emit("progress", step="publish", message="Replacing _site with the new build")
        if SITE_DIR.exists():
            shutil.rmtree(SITE_DIR)
        STAGING_DIR.rename(SITE_DIR)
        file_count = sum(1 for path in SITE_DIR.rglob("*") if path.is_file())
        emit("result", builder="markdown-fallback", output_dir=str(SITE_DIR),
             pages=page_count, files=file_count, message=f"Built {page_count} pages into _site")
        return 0
    except (BuildError, OSError, UnicodeDecodeError) as error:
        emit("error", step="build", message=str(error))
        return 1


if __name__ == "__main__":
    sys.exit(main())
