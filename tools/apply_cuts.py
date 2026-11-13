#!/usr/bin/env python3
"""Apply a cuts file: the hunks a port agent wrote for files it may not edit itself.

A cuts file is Markdown. Each hunk is a `### ` heading followed by three list lines and two
or three fenced blocks:

    ### 3 delete the built-in dialog opener
    - file: `ilium-client/src/app.rs`
    - op: `delete`            (delete | replace | insert-after)
    - removes: `open_create_split_dialog`     (symbols that must be gone afterwards; may be empty)
    ```first
    <1 to 3 consecutive whole lines, verbatim; together they match exactly once in the file>
    ```
    ```last
    <one whole line, verbatim: the first such line at or after the end of `first`>
    ```
    ```with
    <replacement text>
    ```

`delete` removes `first` through `last` inclusive. `replace` puts `with` there instead.
`insert-after` puts `with` after `first`. Without a `last` block the range is `first` alone.
A deleted range must hold whole items: its (), {} and [] must balance. A replaced range must
open and close as many of each as its replacement does.
Hunks are applied in file order, each against the result of the ones before. Nothing is
written unless every hunk of the file applies. Output is JSON lines.

Usage: apply_cuts.py [--check] CUTS.md [CUTS.md ...]   (run from the repository root)
"""

import argparse
import json
import pathlib
import re
import sys

FIELD = re.compile(r"^- (file|op|removes):\s*(.*)$")
OPERATIONS = {"delete", "replace", "insert-after"}


def emit(kind, **fields):
    print(json.dumps({"type": kind, **fields}))


def parse(path):
    hunks, hunk, block = [], None, None
    for number, line in enumerate(path.read_text().splitlines(), 1):
        if block is not None:
            if line.strip() == "```":
                block = None
            else:
                hunk[block].append(line)
            continue
        if line.startswith("### "):
            hunk = {"title": line[4:].strip(), "line": number, "cuts": str(path)}
            hunks.append(hunk)
            continue
        if hunk is None:
            continue
        field = FIELD.match(line)
        if field:
            names = [name.strip("` ") for name in field.group(2).split(",")]
            hunk[field.group(1)] = [name for name in names if name]
        elif line.strip() in ("```first", "```last", "```with"):
            block = line.strip()[3:]
            hunk[block] = []
    return hunks


def find(lines, wanted, start=0):
    return [
        index
        for index in range(start, len(lines) - len(wanted) + 1)
        if lines[index : index + len(wanted)] == wanted
    ]


def balance(removed, added):
    """The bracket pairs whose nesting the exchange would change, as text; empty when sound."""
    before, after = "\n".join(removed), "\n".join(added)
    return ", ".join(
        f"{opening}{closing}"
        for opening, closing in ("()", "{}", "[]")
        if before.count(opening) - before.count(closing)
        != after.count(opening) - after.count(closing)
    )


def apply(hunk, lines):
    """Returns the new lines, or raises ValueError with the reason."""
    operation = (hunk.get("op") or [""])[0]
    if operation not in OPERATIONS:
        raise ValueError(f"op must be one of {sorted(OPERATIONS)}")
    first = hunk.get("first") or []
    if not 1 <= len(first) <= 3:
        raise ValueError("first needs 1 to 3 lines")
    matches = find(lines, first)
    if len(matches) != 1:
        raise ValueError(f"first matches {len(matches)} times, expected exactly 1")
    begin = matches[0]
    end = begin + len(first)
    if "last" in hunk:
        if len(hunk["last"]) != 1:
            raise ValueError("last needs exactly 1 line")
        after = find(lines, hunk["last"], end - 1)
        if not after:
            raise ValueError("last does not occur at or after first")
        end = after[0] + 1
    replacement = hunk.get("with", [])
    if operation != "insert-after":
        added = replacement if operation == "replace" else []
        unbalanced = balance(lines[begin:end], added)
        if unbalanced:
            raise ValueError(
                f"the range (lines {begin + 1} to {end}) leaves {unbalanced} unbalanced: "
                "it does not cover whole items"
            )
    if operation == "delete":
        return lines[:begin] + lines[end:]
    if not replacement:
        raise ValueError(f"{operation} needs a with block")
    if operation == "replace":
        return lines[:begin] + replacement + lines[end:]
    return lines[: begin + len(first)] + replacement + lines[begin + len(first) :]


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--check", action="store_true", help="apply in memory only")
    parser.add_argument("cuts", nargs="+", type=pathlib.Path)
    arguments = parser.parse_args()

    hunks = [hunk for path in arguments.cuts for hunk in parse(path)]
    contents, failed, removed = {}, 0, {}
    for hunk in hunks:
        target = (hunk.get("file") or [""])[0]
        try:
            if target not in contents:
                contents[target] = pathlib.Path(target).read_text().splitlines()
            before = len(contents[target])
            contents[target] = apply(hunk, contents[target])
            emit("progress", hunk=hunk["title"], file=target,
                 line_delta=len(contents[target]) - before)
            removed.setdefault(target, []).extend(hunk.get("removes", []))
        except (OSError, ValueError) as error:
            failed += 1
            emit("error", hunk=hunk["title"], cuts=hunk["cuts"], cuts_line=hunk["line"],
                 file=target, reason=str(error))
    for target, symbols in removed.items():
        text = "\n".join(contents.get(target, []))
        for symbol in symbols:
            if re.search(rf"\b{re.escape(symbol)}\b", text):
                failed += 1
                emit("error", file=target, reason=f"{symbol} is still present after the cuts")
    if not failed and not arguments.check:
        for target, lines in contents.items():
            pathlib.Path(target).write_text("\n".join(lines) + "\n")
    emit("summary", hunks=len(hunks), failed=failed, files=sorted(contents),
         written=not failed and not arguments.check)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
