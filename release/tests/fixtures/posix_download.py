"""Offline curl double. Used only with ILIUM_INSTALL_TEST_ORIGIN by fixtures."""

import json
import os
from pathlib import Path
import sys
import time

arguments = sys.argv[1:]
url = arguments[-1]
root = Path(os.environ["FIXTURE_RELEASE_ROOT"])
with (root / "requests.jsonl").open("a") as log:
    log.write(json.dumps({"type": "request", "url": url, "arguments": arguments}) + "\n")
if not url.startswith("https://127.0.0.1:18443/releases/"):
    sys.exit(91)
if os.environ.get("FIXTURE_DELAY"):
    time.sleep(float(os.environ["FIXTURE_DELAY"]))
failure = os.environ.get("FIXTURE_DOWNLOAD_FAILURE")
if failure in ("network", "rate-limit") or (failure == "asset" and url.endswith(".tar.gz")):
    print({"network": "network unavailable", "rate-limit": "HTTP 429", "asset": "HTTP 404"}[failure], file=sys.stderr)
    sys.exit(22)
if url.endswith("/latest"):
    sys.stdout.write("https://127.0.0.1:18443/releases/tag/v0.1.0")
    sys.exit(0)
parts = url.split("/download/")[-1].split("/")
source = root / parts[0] / parts[1]
if not source.is_file():
    sys.exit(22)
destination = arguments[arguments.index("-o") + 1]
Path(destination).write_bytes(source.read_bytes())
