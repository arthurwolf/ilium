#!/usr/bin/env python3
"""Install ilium release binaries only from a verified ni-build receipt.

Every installed binary must be the exact artifact a successful remote
`cargo build --release` returned: the receipt must report status `done` with
exit code 0, and each local `target/release/<bin>` must match the SHA-256 the
receipt recorded for it. Installation is atomic per binary (copy to a
temporary file, then rename), keeps the previous binary as `<bin>.prev`, and
writes `<bin>.build.json` beside it so the running server can log exactly
which build it is.

Output is JSONL on stdout, one object per line with a `type` field
(`progress`, `artifact`, `error`, `summary`).

Usage:
  tools/install-from-receipt.py --receipt <job-id-or-receipt-dir>
      [--target-dir target] [--bin-dir ~/.cargo/bin]
      [--bin ilium --bin ilium-server ...] [--dry-run]
"""

import argparse
import datetime
import hashlib
import json
import os
import shutil
import sys
from pathlib import Path

DEFAULT_BINARIES = ["ilium", "ilium-server", "ilium-animation-helper"]
# ni-build keeps receipts on the build scratch disk (pruned after 72 h idle).
RECEIPTS_ROOT = Path("/media/arthur/build/ni-build-receipts")


def emit(kind, **fields):
    print(json.dumps({"type": kind, **fields}), flush=True)


def fail(message, **fields):
    emit("error", message=message, **fields)
    emit("summary", ok=False, installed=[])
    sys.exit(1)


def sha256_of(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def load_receipt(reference):
    directory = Path(reference).expanduser()
    if not directory.is_dir():
        directory = RECEIPTS_ROOT / reference
    path = directory / "receipt.json"
    if not path.is_file():
        fail("receipt not found", receipt=str(path))
    with open(path) as handle:
        return path, json.load(handle)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--receipt", required=True, help="ni-build job id or receipt directory")
    parser.add_argument("--target-dir", default="target", help="local Cargo target directory")
    parser.add_argument("--bin-dir", default=str(Path.home() / ".cargo/bin"))
    parser.add_argument("--bin", action="append", dest="binaries", help="binary name (repeatable)")
    parser.add_argument("--dry-run", action="store_true", help="verify only, install nothing")
    arguments = parser.parse_args()
    binaries = arguments.binaries or DEFAULT_BINARIES

    receipt_path, receipt = load_receipt(arguments.receipt)
    command = receipt.get("command") or []
    if receipt.get("status") != "done" or receipt.get("exit_code") != 0:
        fail(
            "receipt is not a successful build",
            receipt=str(receipt_path),
            status=receipt.get("status"),
            exit_code=receipt.get("exit_code"),
        )
    if "build" not in command or "--release" not in command:
        fail("receipt is not a cargo release build", receipt=str(receipt_path), command=command)
    artifacts = {entry["path"]: entry for entry in receipt.get("artifacts") or []}
    emit("progress", message="receipt verified", receipt=str(receipt_path), job_id=receipt.get("job_id"))

    target_dir = Path(arguments.target_dir).expanduser()
    verified = []
    for binary in binaries:
        recorded = artifacts.get(f"release/{binary}")
        local = target_dir / "release" / binary
        if recorded is None:
            fail("binary is not an artifact of this receipt", binary=binary)
        if not local.is_file():
            fail("returned artifact is missing locally", binary=binary, path=str(local))
        actual = sha256_of(local)
        if actual != recorded["sha256"]:
            fail(
                "local artifact differs from the receipt",
                binary=binary,
                path=str(local),
                expected_sha256=recorded["sha256"],
                actual_sha256=actual,
            )
        verified.append((binary, local, actual))
        emit("progress", message="artifact hash verified", binary=binary, sha256=actual)

    if arguments.dry_run:
        emit("summary", ok=True, installed=[], dry_run=True)
        return

    bin_dir = Path(arguments.bin_dir).expanduser()
    bin_dir.mkdir(parents=True, exist_ok=True)
    installed_at = datetime.datetime.now().astimezone().strftime("%Y-%m-%d %H:%M:%S %z")
    installed = []
    for binary, local, digest in verified:
        destination = bin_dir / binary
        temporary = bin_dir / f".{binary}.installing"
        shutil.copyfile(local, temporary)
        os.chmod(temporary, 0o755)
        if sha256_of(temporary) != digest:
            temporary.unlink()
            fail("copied binary does not match its artifact hash", binary=binary)
        previous = None
        if destination.is_file():
            previous = bin_dir / f"{binary}.prev"
            shutil.copy2(destination, previous)
        os.replace(temporary, destination)
        record = {
            "binary": binary,
            "sha256": digest,
            "installed_at": installed_at,
            "receipt": str(receipt_path),
            "job_id": receipt.get("job_id"),
            "host": receipt.get("host"),
            "toolchain": receipt.get("toolchain"),
            "command": command,
        }
        record_path = bin_dir / f"{binary}.build.json"
        record_temporary = bin_dir / f".{binary}.build.json.installing"
        record_temporary.write_text(json.dumps(record, indent=2) + "\n")
        os.replace(record_temporary, record_path)
        installed.append(binary)
        emit(
            "artifact",
            binary=binary,
            path=str(destination),
            sha256=digest,
            previous=str(previous) if previous else None,
            build_record=str(record_path),
        )
    emit("summary", ok=True, installed=installed, job_id=receipt.get("job_id"))


if __name__ == "__main__":
    main()
