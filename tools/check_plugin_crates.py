#!/usr/bin/env python3
"""Checks the crates every core plugin depends on against the locked allowlist (gap G18).

For each `plugins/core/*/Cargo.toml` (or the manifests named by --plugins-glob) the tool checks
that every normal and build dependency

  * has an entry in `Cargo.lock` whose version satisfies the requirement,
  * is listed in `tools/plugin_crate_allowlist.json` with `wasm_safe` true, the resolved version
    in the entry's `versions`, and features that satisfy `default_features`,
    `required_features` and `forbidden_features`.

Dev-dependencies run natively in tests: they only have to be in `Cargo.lock`. Path dependencies
on a workspace crate are checked like any other crate (the SDK is in the allowlist).

Output is JSONL on stdout, one object per line: `violation` rows, one `summary` row and a final
`result` row. Exit 0 when there is no violation, 1 on a violation, 2 on a usage or input error.

Run it with `python3 -I tools/check_plugin_crates.py`.
"""

import argparse
import json
import re
import sys
import tomllib
from pathlib import Path

DEPENDENCY_TABLES = ("dependencies", "build-dependencies")
NATIVE_ONLY_TABLES = ("dev-dependencies",)


def emit(row):
    print(json.dumps(row, sort_keys=True))


def parse_version(text):
    """`1.2.3` to (1, 2, 3); missing parts are 0, a pre-release suffix is dropped."""
    parts = re.split(r"[.+-]", text.strip())
    numbers = []
    for part in parts[:3]:
        if not part.isdigit():
            break
        numbers.append(int(part))
    while len(numbers) < 3:
        numbers.append(0)
    return tuple(numbers)


def satisfies_comparator(version, comparator):
    """One Cargo comparator: `^1.2`, `~1.2.3`, `=1.2.3`, `>=1.2`, `<2`, `1.2.3` (caret)."""
    comparator = comparator.strip()
    if comparator in ("", "*"):
        return True
    match = re.match(r"^(\^|~|=|>=|<=|>|<)?\s*(\d.*)$", comparator)
    if not match:
        return False
    operator = match.group(1) or "^"
    written = match.group(2)
    bound = parse_version(written)
    given = len([p for p in re.split(r"[.]", written.split("-")[0]) if p != ""])
    if operator == "=":
        return version == bound if given >= 3 else version[:given] == bound[:given]
    if operator == ">=":
        return version >= bound
    if operator == "<=":
        return version <= bound
    if operator == ">":
        return version > bound
    if operator == "<":
        return version < bound
    if operator == "~":
        upper = (bound[0], bound[1] + 1, 0) if given >= 2 else (bound[0] + 1, 0, 0)
        return bound <= version < upper
    # Caret: the leftmost non-zero component is the compatibility boundary.
    if bound[0] > 0 or given == 1:
        upper = (bound[0] + 1, 0, 0)
    elif bound[1] > 0 or given == 2:
        upper = (0, bound[1] + 1, 0)
    else:
        upper = (0, 0, bound[2] + 1)
    return bound <= version < upper


def satisfies(version_text, requirement):
    version = parse_version(version_text)
    return all(satisfies_comparator(version, part) for part in requirement.split(","))


def locked_versions(lock_path):
    """Crate name to the list of locked versions."""
    with open(lock_path, "rb") as handle:
        lock = tomllib.load(handle)
    versions = {}
    for package in lock.get("package", []):
        versions.setdefault(package["name"], []).append(package["version"])
    return versions


def dependency_facts(name, spec):
    """(requirement, default_features, features, package_name) of one dependency entry."""
    if isinstance(spec, str):
        return spec, True, [], name
    return (
        spec.get("version", "*"),
        spec.get("default-features", spec.get("default_features", True)),
        list(spec.get("features", [])),
        spec.get("package", name),
    )


def check_dependency(plugin, table, name, spec, locked, allowlist):
    requirement, default_features, features, package = dependency_facts(name, spec)
    base = {"type": "violation", "plugin": plugin, "table": table, "crate": package}
    problems = []
    candidates = [v for v in locked.get(package, []) if satisfies(v, requirement)]
    if not candidates:
        problems.append(
            {
                "rule": "not-in-lock",
                "detail": f"no Cargo.lock entry of {package} satisfies {requirement!r}",
            }
        )
    if table in NATIVE_ONLY_TABLES:
        return [{**base, **problem} for problem in problems]
    entry = allowlist.get(package)
    if entry is None:
        problems.append(
            {"rule": "not-allowed", "detail": f"{package} is not in the plugin crate allowlist"}
        )
        return [{**base, **problem} for problem in problems]
    if not entry.get("wasm_safe", False):
        problems.append(
            {"rule": "not-wasm-safe", "detail": entry.get("note", "marked wasm_safe false")}
        )
    allowed = entry.get("versions", [])
    if candidates and not any(version in allowed for version in candidates):
        problems.append(
            {
                "rule": "version-not-allowed",
                "detail": f"locked {sorted(candidates)} not in allowed {allowed}",
            }
        )
    wanted_default = entry.get("default_features")
    if wanted_default is not None and default_features != wanted_default:
        problems.append(
            {
                "rule": "default-features",
                "detail": f"default-features must be {str(wanted_default).lower()}",
            }
        )
    missing = [f for f in entry.get("required_features", []) if f not in features]
    if missing:
        problems.append({"rule": "required-features", "detail": f"missing features {missing}"})
    forbidden = [f for f in entry.get("forbidden_features", []) if f in features]
    if forbidden:
        problems.append({"rule": "forbidden-features", "detail": f"forbidden features {forbidden}"})
    return [{**base, **problem} for problem in problems]


def check_manifest(manifest_path, locked, allowlist):
    with open(manifest_path, "rb") as handle:
        manifest = tomllib.load(handle)
    plugin = manifest.get("package", {}).get("name", manifest_path.parent.name)
    violations = []
    checked = 0
    for table in DEPENDENCY_TABLES + NATIVE_ONLY_TABLES:
        for name, spec in sorted(manifest.get(table, {}).items()):
            checked += 1
            violations.extend(check_dependency(plugin, table, name, spec, locked, allowlist))
    return checked, violations


def run(root, allowlist_path, lock_path, plugins_glob):
    try:
        with open(allowlist_path, encoding="utf-8") as handle:
            allowlist = {k: v for k, v in json.load(handle).items() if not k.startswith("_")}
        locked = locked_versions(lock_path)
    except (OSError, ValueError, tomllib.TOMLDecodeError) as error:
        emit({"type": "error", "detail": f"cannot read inputs: {error}"})
        return 2
    manifests = sorted(root.glob(plugins_glob))
    if not manifests:
        emit({"type": "error", "detail": f"no manifest matches {plugins_glob} under {root}"})
        return 2
    total = 0
    violations = []
    for manifest_path in manifests:
        try:
            checked, found = check_manifest(manifest_path, locked, allowlist)
        except (OSError, tomllib.TOMLDecodeError) as error:
            emit({"type": "error", "detail": f"cannot read {manifest_path}: {error}"})
            return 2
        total += checked
        violations.extend(found)
    for violation in violations:
        emit(violation)
    emit(
        {
            "type": "summary",
            "plugins": len(manifests),
            "dependencies": total,
            "violations": len(violations),
        }
    )
    emit({"type": "result", "exit_code": 1 if violations else 0})
    return 1 if violations else 0


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--root", default=".", help="workspace root (default: current directory)")
    parser.add_argument("--allowlist", default=None, help="allowlist JSON (default: tools/...)")
    parser.add_argument("--lock", default=None, help="Cargo.lock (default: <root>/Cargo.lock)")
    parser.add_argument(
        "--plugins-glob", default="plugins/core/*/Cargo.toml", help="manifest glob under root"
    )
    args = parser.parse_args(argv)
    root = Path(args.root).resolve()
    allowlist = Path(args.allowlist) if args.allowlist else root / "tools/plugin_crate_allowlist.json"
    lock = Path(args.lock) if args.lock else root / "Cargo.lock"
    return run(root, allowlist, lock, args.plugins_glob)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
