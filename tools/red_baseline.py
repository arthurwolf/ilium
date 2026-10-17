#!/usr/bin/env python3
"""Parse a `cargo test --no-fail-fast` log into the red-baseline record (test-plan section 6.4).

Usage:
  red_baseline.py --test-log LOG [--allowlist FILE] [--root DIR] [--out FILE]
  red_baseline.py --compare BEFORE.jsonl AFTER.jsonl

--test-log reads `Running <target>` and `test <name> ... ok|FAILED|ignored` lines (stable cargo has no JSON
test output), writes one JSON object per test plus a summary record, and exits 1 when a test passed that is
not allow-listed or a target failed to build. --compare checks that the sets of passing tests are equal
(section 6.5: the skeleton must not change existing results) and exits 1 on any difference.
"""
import argparse
import json
import re
import sys

RUNNING = re.compile(r"^\s*Running (?:unittests )?(\S.*?)(?: \((.*)\))?\s*$")
DOC_TESTS = re.compile(r"^\s*Doc-tests (\S+)")
RESULT = re.compile(r"^test (.+?) \.\.\. (ok|FAILED|ignored)(?:, .*)?$")
BUILD_FAILURE = re.compile(r"^error(\[E\d+\])?: could not compile `([^`]+)`")


def parse_log(path):
    tests, build_failures = [], []
    target = crate = None
    with open(path, errors="replace") as handle:
        for line in handle:
            line = line.rstrip("\n")
            running = RUNNING.match(line)
            if running:
                target, crate = running.group(1), (running.group(2) or running.group(1))
                continue
            doc = DOC_TESTS.match(line)
            if doc:
                target, crate = "doc-tests", doc.group(1)
                continue
            failure = BUILD_FAILURE.match(line)
            if failure:
                build_failures.append(failure.group(2))
                continue
            result = RESULT.match(line)
            if result and target is not None:
                status = {"ok": "passed", "FAILED": "failed", "ignored": "ignored"}[result.group(2)]
                tests.append({"type": "test", "crate": crate, "target": target, "name": result.group(1), "status": status})
    return tests, sorted(set(build_failures))


def read_allowlist(path):
    """Exact `name` / `target::name` entries, plus `re:<regex>` lines matched against the bare test name."""
    if path is None:
        return set()
    try:
        with open(path) as handle:
            return {entry.strip() for entry in handle if entry.strip() and not entry.startswith("#")}
    except OSError:
        return set()


def is_allowed(entry, allow):
    if entry["name"] in allow or f"{entry['target']}::{entry['name']}" in allow:
        return True
    return any(item.startswith("re:") and re.search(item[3:], entry["name"]) for item in allow)


def attach_ids(tests, root):
    """Best effort: REQ ids are not in the log, so leave `ids` empty unless a source scan is added later."""
    for entry in tests:
        entry["ids"] = []


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--test-log")
    parser.add_argument("--allowlist")
    parser.add_argument("--root")
    parser.add_argument("--out")
    parser.add_argument("--compare", nargs=2, metavar=("BEFORE", "AFTER"))
    args = parser.parse_args()

    if args.compare:
        sets = []
        for path in args.compare:
            with open(path) as handle:
                records = [json.loads(line) for line in handle if line.strip()]
            sets.append({(r["target"], r["name"]) for r in records if r.get("type") == "test" and r["status"] == "passed"})
        before, after = sets
        lost, gained = sorted(before - after), sorted(after - before)
        print(json.dumps({"type": "compare", "before_passed": len(before), "after_passed": len(after),
                          "lost": lost[:200], "lost_count": len(lost), "gained_count": len(gained), "gained": gained[:50]}))
        return 0 if not lost and not gained else 1

    if not args.test_log:
        parser.error("--test-log or --compare is required")
    tests, build_failures = parse_log(args.test_log)
    attach_ids(tests, args.root)
    allow = read_allowlist(args.allowlist)
    unexpected_pass = [t for t in tests if t["status"] == "passed" and not is_allowed(t, allow)]
    counts = {}
    for entry in tests:
        counts[entry["status"]] = counts.get(entry["status"], 0) + 1
    lines = [json.dumps(entry) for entry in tests]
    summary = {"type": "summary", "counts": counts, "build_failures": build_failures,
               "unexpected_pass": len(unexpected_pass)}
    lines.append(json.dumps(summary))
    if args.out:
        with open(args.out, "w") as handle:
            handle.write("\n".join(lines) + "\n")
    else:
        print("\n".join(lines))
    print(json.dumps(summary), file=sys.stderr)
    return 1 if unexpected_pass or build_failures else 0


if __name__ == "__main__":
    sys.exit(main())
