#!/usr/bin/env python3
"""Extract the WIT contract from the plugin manual into wit/.

The WIT reference page (docs/plugins-manual/pages/reference-wit.md) is the only
canonical definition of the plugin contract (ABI-001). This tool copies every
```wit fenced block of that page, byte for byte, into the layout of the test
plan:

    wit/types/ilium-types.wit          package ilium:types@0.1.0 (all interfaces)
    wit/host/<interface>.wit           package ilium:host@0.1.0, one file per interface
    wit/primitives/<interface>.wit     package ilium:primitives@0.1.0
    wit/anim/<interface>.wit           package ilium:anim@0.1.0 (interfaces)
    wit/exports/<interface>.wit        package ilium:exports@0.1.0
    wit/worlds/{server,client,anim}.wit  the world packages
    wit/EXTRACTED_FROM                 page path and sha256 of the page

Every per-interface file starts with the package line of its package, so each
file parses on its own. The text of each block is never changed.

Python 3 standard library only. Output is JSONL on stdout, one object per line,
each with a "type" field. Exit codes: 0 success (or no drift with --check),
1 drift found with --check, 2 usage or parse error.
"""

import argparse
import hashlib
import json
import pathlib
import re
import sys

PAGE_RELATIVE = "docs/plugins-manual/pages/reference-wit.md"
EXPECTED_BLOCKS = 83
EXPECTED_INTERFACES = 76
EXPECTED_WORLD_FILES = 3
PACKAGE_DIRECTORIES = {
    "ilium:types": "types",
    "ilium:host": "host",
    "ilium:primitives": "primitives",
    "ilium:anim": "anim",
    "ilium:exports": "exports",
}
WORLD_FILES = {
    "ilium:server": "worlds/server.wit",
    "ilium:client": "worlds/client.wit",
    "ilium:anim": "worlds/anim.wit",
}
PACKAGE_LINE = re.compile(r"^package (ilium:[a-z-]+)@(\d+\.\d+\.\d+);\s*$")
INTERFACE_LINE = re.compile(r"^interface ([a-z][a-z0-9-]*) \{")
WORLD_LINE = re.compile(r"^world ([a-z][a-z0-9-]*) \{")


def emit(record):
    """Print one JSONL record."""
    print(json.dumps(record, sort_keys=True))


class ExtractError(Exception):
    """A structural problem in the page that stops the extraction."""


def read_blocks(page_text):
    """Return the list of (start_line, text) of every ```wit block, text without fences."""
    blocks = []
    lines = page_text.split("\n")
    inside = False
    start_line = 0
    current = []
    for line_number, line in enumerate(lines, start=1):
        if not inside:
            if line.strip() == "```wit":
                inside = True
                start_line = line_number + 1
                current = []
            continue
        if line.strip() == "```":
            inside = False
            blocks.append((start_line, "\n".join(current)))
            continue
        current.append(line)
    if inside:
        raise ExtractError(f"unterminated ```wit block starting at line {start_line}")
    return blocks


def plan_files(blocks):
    """Map the blocks to output files. Returns {relative path: text} in declaration order."""
    files = {}
    current_package = None
    current_version = None
    types_parts = []
    interface_count = 0
    world_files = 0
    for start_line, text in blocks:
        block_lines = text.split("\n")
        package_lines = [line for line in block_lines if PACKAGE_LINE.match(line)]
        interface_names = [INTERFACE_LINE.match(line).group(1) for line in block_lines if INTERFACE_LINE.match(line)]
        world_names = [WORLD_LINE.match(line).group(1) for line in block_lines if WORLD_LINE.match(line)]
        if len(package_lines) > 1:
            raise ExtractError(f"block at line {start_line} declares more than one package")
        if package_lines:
            match = PACKAGE_LINE.match(package_lines[0])
            current_package = match.group(1)
            current_version = match.group(2)
        if current_package is None:
            raise ExtractError(f"block at line {start_line} comes before any package line")
        if world_names:
            if interface_names:
                raise ExtractError(f"block at line {start_line} mixes worlds and interfaces")
            if not package_lines:
                raise ExtractError(f"world block at line {start_line} has no package line")
            relative = WORLD_FILES.get(current_package)
            if relative is None:
                raise ExtractError(f"world package {current_package} has no output file")
            if relative in files:
                raise ExtractError(f"world package {current_package} appears twice")
            files[relative] = text.rstrip("\n") + "\n"
            world_files += 1
            continue
        if not interface_names:
            # A block that only switches the package (for example "package ilium:host@0.1.0;").
            if not package_lines or any(line.strip() and not PACKAGE_LINE.match(line) for line in block_lines):
                raise ExtractError(f"block at line {start_line} has neither an interface nor a world")
            continue
        if len(interface_names) != 1:
            raise ExtractError(f"block at line {start_line} declares {len(interface_names)} interfaces; expected 1")
        interface_count += 1
        directory = PACKAGE_DIRECTORIES.get(current_package)
        if directory is None:
            raise ExtractError(f"interface package {current_package} has no output directory")
        if current_package == "ilium:types":
            types_parts.append(text.rstrip("\n"))
            continue
        relative = f"{directory}/{interface_names[0]}.wit"
        if relative in files:
            raise ExtractError(f"interface {current_package}/{interface_names[0]} appears twice")
        package_line = f"package {current_package}@{current_version};"
        body = text.rstrip("\n")
        if package_lines:
            files[relative] = body + "\n"
        else:
            files[relative] = package_line + "\n\n" + body + "\n"
    if types_parts:
        files["types/ilium-types.wit"] = "\n\n".join(types_parts) + "\n"
    return files, interface_count, world_files


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    default_root = pathlib.Path(__file__).resolve().parent.parent
    parser.add_argument("--root", type=pathlib.Path, default=default_root, help="workspace root (default: parent of tools/)")
    parser.add_argument("--manual", type=pathlib.Path, default=None, help="manual pages directory (default: <root>/docs/plugins-manual/pages)")
    parser.add_argument("--check", action="store_true", help="compare with wit/ instead of writing; exit 1 on drift")
    try:
        arguments = parser.parse_args(argv)
    except SystemExit as exit_request:
        return 2 if exit_request.code else 0
    root = arguments.root.resolve()
    manual = (arguments.manual or root / "docs/plugins-manual/pages").resolve()
    page = manual / "reference-wit.md"
    if not page.is_file():
        emit({"type": "error", "code": "page-missing", "path": str(page)})
        return 2
    page_bytes = page.read_bytes()
    page_text = page_bytes.decode("utf-8")
    try:
        blocks = read_blocks(page_text)
        if len(blocks) != EXPECTED_BLOCKS:
            raise ExtractError(f"found {len(blocks)} wit blocks; expected {EXPECTED_BLOCKS}")
        files, interface_count, world_files = plan_files(blocks)
        if interface_count != EXPECTED_INTERFACES:
            raise ExtractError(f"found {interface_count} interfaces; expected {EXPECTED_INTERFACES}")
        if world_files != EXPECTED_WORLD_FILES:
            raise ExtractError(f"found {world_files} world packages; expected {EXPECTED_WORLD_FILES}")
    except ExtractError as error:
        emit({"type": "error", "code": "parse", "message": str(error)})
        return 2
    page_sha256 = hashlib.sha256(page_bytes).hexdigest()
    files["EXTRACTED_FROM"] = (
        f"source: {PAGE_RELATIVE}\n"
        f"sha256: {page_sha256}\n"
        f"blocks: {len(blocks)}\n"
        "tool: tools/extract_wit.py\n"
    )
    wit_directory = root / "wit"
    expected_paths = {wit_directory / relative for relative in files}
    stale = sorted(path for path in wit_directory.rglob("*.wit") if path not in expected_paths) if wit_directory.is_dir() else []
    drift = []
    for relative, text in files.items():
        target = wit_directory / relative
        data = text.encode("utf-8")
        current = target.read_bytes() if target.is_file() else None
        if current == data:
            emit({"type": "artifact", "path": str(target), "status": "unchanged", "sha256": hashlib.sha256(data).hexdigest()})
            continue
        drift.append(relative)
        if arguments.check:
            emit({"type": "warning", "code": "drift", "path": str(target)})
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        emit({"type": "artifact", "path": str(target), "status": "written", "sha256": hashlib.sha256(data).hexdigest()})
    for path in stale:
        drift.append(str(path.relative_to(wit_directory)))
        if arguments.check:
            emit({"type": "warning", "code": "stale-file", "path": str(path)})
        else:
            path.unlink()
            emit({"type": "artifact", "path": str(path), "status": "removed"})
    emit({
        "type": "summary",
        "page": str(page),
        "page_sha256": page_sha256,
        "blocks": len(blocks),
        "interfaces": interface_count,
        "world_files": world_files,
        "files": len(files),
        "drift": len(drift),
        "mode": "check" if arguments.check else "write",
    })
    if arguments.check and drift:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
