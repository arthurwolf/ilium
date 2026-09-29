"""Release policy and deterministic paired archives; stdout is always JSONL.

Archive validation never executes or extracts members. Native execution belongs
to audit_native.py; its receipt binds version output to the exact member hashes.
"""

import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import re
import stat
import sys
import tarfile
import tempfile
import tomllib
import zipfile


TARGET_FIELDS = frozenset(
    ("os", "arch", "rust_target", "runner", "archive", "format",
     "executables", "ort_strategy", "minimum_tested_os")
)
# This is an acceptance constraint, not a second generated release matrix.
# The manifest supplies the records consumed by packaging and installers.
APPROVED_PAIRS = frozenset(
    ("linux", architecture) for architecture in ("x86_64", "aarch64")
) | frozenset(
    ("macos", architecture) for architecture in ("x86_64", "aarch64")
) | {("windows", "x86_64")}


class ReleaseError(ValueError):
    """Invalid CLI input or release policy."""


def emit(record):
    print(json.dumps(record, ensure_ascii=True, separators=(",", ":")))


class JsonArgumentParser(argparse.ArgumentParser):
    def error(self, message):
        raise ReleaseError(message)

    def print_help(self, file=None):
        if file is None or file is sys.stdout:
            emit({"type": "result", "command": "help", "help": self.format_help()})
        else:
            super().print_help(file)


def validate_target(target, index):
    location = f"target[{index}]"
    if not isinstance(target, dict):
        raise ReleaseError(f"{location} must be a table")
    if set(target) != TARGET_FIELDS:
        missing = sorted(TARGET_FIELDS - set(target))
        unknown = sorted(set(target) - TARGET_FIELDS)
        raise ReleaseError(f"{location} fields differ: missing={missing}, unknown={unknown}")
    for field in TARGET_FIELDS - {"executables"}:
        if not isinstance(target[field], str) or not target[field]:
            raise ReleaseError(f"{location}.{field} must be a non-empty string")
    executables = target["executables"]
    if not isinstance(executables, list) or not all(isinstance(value, str) for value in executables):
        raise ReleaseError(f"{location}.executables must be an array of strings")
    archive = target["archive"]
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", archive) or ".." in archive:
        raise ReleaseError(f"{location}.archive is not a safe basename")
    return dict(target)


def validate_policy(target):
    operating_system, architecture = target["os"], target["arch"]
    if (operating_system, architecture) not in APPROVED_PAIRS:
        raise ReleaseError(f"unsupported target {operating_system}/{architecture}")
    target_suffix = {
        "linux": "unknown-linux-gnu", "windows": "pc-windows-msvc", "macos": "apple-darwin",
    }[operating_system]
    runner = {"linux": "ubuntu-22.04", "windows": "windows-2022", "macos": "macos-15"}[operating_system]
    if operating_system == "linux" and architecture == "aarch64":
        runner += "-arm"
    if operating_system == "macos" and architecture == "x86_64":
        runner += "-intel"
    archive_format = "zip" if operating_system == "windows" else "tar.gz"
    executable_suffix = ".exe" if operating_system == "windows" else ""
    expected = {
        "rust_target": f"{architecture}-{target_suffix}",
        "runner": runner,
        "archive": f"ilium-{operating_system}-{architecture}.{archive_format}",
        "format": archive_format,
        "executables": [f"ilium{executable_suffix}", f"ilium-server{executable_suffix}"],
        "ort_strategy": "pinned-source-build" if (operating_system, architecture) == ("macos", "x86_64") else "upstream-prebuilt",
        "minimum_tested_os": runner,
    }
    for field, value in expected.items():
        if target[field] != value:
            raise ReleaseError(f"{operating_system}/{architecture}.{field} must equal {value!r}")


def load_targets(manifest):
    """Read an explicit TOML path and reject any deviation from release policy."""
    with Path(manifest).open("rb") as manifest_file:
        document = tomllib.load(manifest_file)
    if set(document) != {"target"}:
        raise ReleaseError("manifest must contain only [[target]] records")
    if not isinstance(document["target"], list):
        raise ReleaseError("target must be an array of tables")
    targets = [validate_target(target, index) for index, target in enumerate(document["target"])]
    seen_pairs = set()
    seen_fields = {field: set() for field in ("rust_target", "runner", "archive")}
    for target in targets:
        pair = target["os"], target["arch"]
        if pair in seen_pairs:
            raise ReleaseError(f"duplicate OS/architecture {pair[0]}/{pair[1]}")
        seen_pairs.add(pair)
        for field, seen in seen_fields.items():
            if target[field] in seen:
                raise ReleaseError(f"duplicate {field}: {target[field]}")
            seen.add(target[field])
        validate_policy(target)
    if seen_pairs != APPROVED_PAIRS:
        raise ReleaseError("release matrix must contain exactly the five approved targets")
    return targets


def digest(content):
    return hashlib.sha256(content).hexdigest()


def read_json(path):
    def unique_keys(pairs):
        document = {}
        for key, value in pairs:
            if key in document:
                raise ReleaseError(f"duplicate JSON key: {key}")
            document[key] = value
        return document
    document = json.loads(Path(path).read_text(encoding="utf-8"), object_pairs_hook=unique_keys)
    if not isinstance(document, dict):
        raise ReleaseError("JSON input must be an object")
    return document


def workspace_version(workspace, tag):
    with Path(workspace).open("rb") as source:
        document = tomllib.load(source)
    version = document.get("workspace", {}).get("package", {}).get("version")
    if not isinstance(version, str) or not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?", version):
        raise ReleaseError("workspace version is not a safe semantic version")
    if tag != f"v{version}":
        raise ReleaseError("tag and workspace version differ")
    return version


def selected_target(manifest, rust_target):
    targets = load_targets(manifest)
    matches = [target for target in targets if target["rust_target"] == rust_target]
    if len(matches) != 1:
        raise ReleaseError("target is absent from the approved release matrix")
    return matches[0]


def safe_member_name(name):
    if not isinstance(name, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", name) or ".." in name:
        raise ReleaseError(f"unsafe member basename: {name!r}")
    return name


def audit_receipt(path, target, version, tag):
    receipt = read_json(path)
    if receipt.get("schema") != 1 or receipt.get("state") != "passed" or receipt.get("publication_allowed") is not True:
        raise ReleaseError("native audit is blocked or incomplete; publication prohibited")
    for key, value in (("target", target["rust_target"]), ("tag", tag), ("version", version), ("os", target["os"]), ("arch", target["arch"])):
        if receipt.get(key) != value:
            raise ReleaseError(f"native audit {key} differs from package identity")
    for key in ("native_identity", "dependency_closure", "binary_versions", "notices"):
        if not isinstance(receipt.get(key), dict):
            raise ReleaseError(f"native audit {key} must be an object")
    if target["os"] == "macos":
        for key in ("loader_paths", "embedding", "signing", "notarization"):
            if not isinstance(receipt.get(key), dict):
                raise ReleaseError(f"native audit {key} must be an object")
    native = receipt.get("native_identity", {})
    systems = {"linux": "Linux", "windows": "Windows", "macos": "Darwin"}
    machines = {"x86_64": {"x86_64", "AMD64"}, "aarch64": {"aarch64", "arm64", "ARM64"}}
    if native.get("system") != systems[target["os"]] or native.get("machine") not in machines[target["arch"]] or not native.get("runner"):
        raise ReleaseError("native audit runner does not match target")
    if receipt.get("dependency_closure", {}).get("complete") is not True:
        raise ReleaseError("native dependency closure is incomplete")
    files = receipt.get("files")
    if not isinstance(files, dict) or not files or len(files) > 256:
        raise ReleaseError("native audit has no bounded member hash inventory")
    for name, sha256 in files.items():
        safe_member_name(name)
        if not isinstance(sha256, str) or not re.fullmatch(r"[0-9a-f]{64}", sha256):
            raise ReleaseError(f"invalid audit SHA-256 for {name}")
    required = {*target["executables"], "VERSION", "THIRD-PARTY.txt"}
    if not required <= set(files):
        raise ReleaseError("audit inventory is missing the matched pair, VERSION or notices")
    for executable in target["executables"]:
        label = executable.removesuffix(".exe")
        if receipt.get("binary_versions", {}).get(executable) != f"{label} {version}":
            raise ReleaseError(f"native output for {executable} differs from version")
    notices = receipt.get("notices", {})
    if notices.get("state") != "reviewed" or notices.get("sha256") != files["THIRD-PARTY.txt"]:
        raise ReleaseError("third-party notices are not reviewed and bound to these bytes")
    if target["os"] == "macos":
        if receipt.get("loader_paths", {}).get("state") != "passed" or receipt.get("embedding", {}).get("state") != "passed":
            raise ReleaseError("macOS requires native loader and post-install embedding proof")
        if receipt.get("signing", {}).get("state") not in ("unsigned", "verified"):
            raise ReleaseError("macOS signing state is missing or unverified")
        if receipt.get("notarization", {}).get("state") not in ("disabled", "verified"):
            raise ReleaseError("macOS notarization state is missing or unverified")
        if target["arch"] == "x86_64":
            ort = receipt.get("intel_ort", {})
            if ort.get("state") != "passed" or ort.get("source_tag") != "v1.24.2" or ort.get("source_commit") != "058787ceead760166e3c50a0a4cba8a833a6f53f" or not re.fullmatch(r"[0-9a-f]{64}", ort.get("source_sha256", "")):
                raise ReleaseError("Intel package lacks pinned native ORT source provenance")
    return receipt


def verify_content(content, receipt, version):
    if set(content) != set(receipt["files"]):
        raise ReleaseError("archive member inventory differs from native audited inventory")
    for name, data in content.items():
        if digest(data) != receipt["files"][name]:
            raise ReleaseError(f"member hash differs from native audit: {name}")
    if content["VERSION"] != (version + "\n").encode():
        raise ReleaseError("VERSION differs from tag/workspace/native binary identity")
    notices = content["THIRD-PARTY.txt"].decode("utf-8")
    if not notices.strip() or any(marker in notices.casefold() for marker in ("state: blocked", '"state": "blocked"', "audit later", "placeholder", "todo licence", "todo license")):
        raise ReleaseError("third-party provenance is blocked or notices are a template")


def archive_prefix(target):
    return target["archive"].removesuffix("." + target["format"])


def write_archive(path, target, content):
    prefix = archive_prefix(target)
    entries = [(prefix, None)] + [(f"{prefix}/{name}", data) for name, data in sorted(content.items())]
    if target["format"] == "tar.gz":
        with Path(path).open("wb") as raw:
            with gzip.GzipFile(filename="", fileobj=raw, mode="wb", mtime=0) as compressed:
                with tarfile.open(fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT) as archive:
                    for name, data in entries:
                        member = tarfile.TarInfo(name)
                        member.type = tarfile.DIRTYPE if data is None else tarfile.REGTYPE
                        member.mode = 0o755 if data is None or name.rsplit("/", 1)[-1] in target["executables"] else 0o644
                        member.size = len(data) if data is not None else 0
                        archive.addfile(member, io.BytesIO(data) if data is not None else None)
    else:
        with zipfile.ZipFile(path, "w", compression=zipfile.ZIP_STORED) as archive:
            for name, data in entries:
                member = zipfile.ZipInfo(name + "/" if data is None else name, date_time=(1980, 1, 1, 0, 0, 0))
                member.create_system = 3
                mode = 0o755 if data is None or name.rsplit("/", 1)[-1] in target["executables"] else 0o644
                member.external_attr = ((stat.S_IFDIR if data is None else stat.S_IFREG) | mode) << 16
                if data is None:
                    member.external_attr |= 0x10
                archive.writestr(member, data or b"")


def read_archive(path, target, receipt):
    prefix = archive_prefix(target)
    expected = [prefix] + [f"{prefix}/{name}" for name in sorted(receipt["files"])]
    content, names = {}, []
    total_size = 0
    if target["format"] == "tar.gz":
        with Path(path).open("rb") as raw:
            header = raw.read(10)
        if len(header) != 10 or header[:4] != b"\x1f\x8b\x08\x00" or header[4:8] != b"\0\0\0\0":
            raise ReleaseError("gzip header is not deterministic")
        with tarfile.open(path, "r:gz") as archive:
            end = 0
            for member in archive:
                if member.offset != end:
                    raise ReleaseError("tar contains hidden headers or gaps")
                end = member.offset_data + ((member.size + 511) // 512) * 512
                names.append(member.name)
                if len(names) > len(expected) or member.name != expected[len(names) - 1]:
                    raise ReleaseError("unsafe, duplicate, unexpected or unordered archive member")
                directory = len(names) == 1
                if (directory and not member.isdir()) or (not directory and not member.isreg()) or member.linkname or member.pax_headers:
                    raise ReleaseError("links, special files and extended metadata are prohibited")
                name = member.name.rsplit("/", 1)[-1]
                mode = 0o755 if directory or name in target["executables"] else 0o644
                if member.mode != mode or member.mtime != 0 or member.uid != 0 or member.gid != 0 or member.uname or member.gname:
                    raise ReleaseError("tar metadata is not normalized")
                if not directory:
                    total_size += member.size
                    if member.size < 0 or total_size > 2_000_000_000:
                        raise ReleaseError("archive exceeds bounded size")
                    source = archive.extractfile(member)
                    if source is None:
                        raise ReleaseError("archive regular member is unreadable")
                    content[name] = source.read()
            padding_size = ((end + 1024 + tarfile.RECORDSIZE - 1) // tarfile.RECORDSIZE) * tarfile.RECORDSIZE - end
            archive.fileobj.seek(end)
            if archive.fileobj.read(padding_size + 1) != b"\0" * padding_size:
                raise ReleaseError("tar terminator contains unexpected trailing bytes")
    else:
        with zipfile.ZipFile(path) as archive:
            local_end, central_size = 0, 0
            if archive.comment:
                raise ReleaseError("zip archive comment is not normalized")
            for member in archive.infolist():
                if member.header_offset != local_end or member.compress_size != member.file_size:
                    raise ReleaseError("zip has hidden local members or gaps")
                local_end += 30 + len(member.filename.encode("ascii")) + member.file_size
                central_size += 46 + len(member.filename.encode("ascii"))
                directory = len(names) == 0
                name = member.filename.removesuffix("/") if directory else member.filename
                names.append(name)
                if len(names) > len(expected) or name != expected[len(names) - 1]:
                    raise ReleaseError("unsafe, duplicate, unexpected or unordered archive member")
                basename = name.rsplit("/", 1)[-1]
                mode = 0o755 if directory or basename in target["executables"] else 0o644
                expected_attributes = ((stat.S_IFDIR if directory else stat.S_IFREG) | mode) << 16
                if directory:
                    expected_attributes |= 0x10
                if member.external_attr != expected_attributes or member.create_system != 3 or member.date_time != (1980, 1, 1, 0, 0, 0) or member.extra or member.comment or member.flag_bits or member.compress_type != zipfile.ZIP_STORED:
                    raise ReleaseError("zip metadata, links or encryption are not permitted")
                if directory and (not member.is_dir() or member.file_size != 0):
                    raise ReleaseError("archive root must be an empty directory entry")
                total_size += member.file_size
                if total_size > 2_000_000_000:
                    raise ReleaseError("archive exceeds bounded size")
                if not directory:
                    content[basename] = archive.read(member)
            if archive.start_dir != local_end or Path(path).stat().st_size != local_end + central_size + 22:
                raise ReleaseError("zip contains unexpected prefix/trailer or directory metadata")
    if names != expected:
        raise ReleaseError("archive is missing required members")
    return content


def validate_checksums(path, targets, archive):
    checksums = {}
    for line in Path(path).read_text(encoding="ascii").splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9._-]+)", line)
        if not match or match[2] in checksums:
            raise ReleaseError("malformed or duplicate checksum entry")
        checksums[match[2]] = match[1]
    if set(checksums) != {target["archive"] for target in targets}:
        raise ReleaseError("checksum inventory must cover exactly all five archives")
    if digest(Path(archive).read_bytes()) != checksums[Path(archive).name]:
        raise ReleaseError("archive checksum differs from final bytes")


def package(arguments):
    target = selected_target(arguments.manifest, arguments.target)
    version = workspace_version(arguments.workspace, arguments.tag)
    receipt = audit_receipt(arguments.audit_report, target, version, arguments.tag)
    output = arguments.output.resolve()
    if output.name != target["archive"] or arguments.output.is_symlink():
        raise ReleaseError("output archive name differs from approved target or is a link")
    directory = arguments.directory
    if directory.is_symlink() or not directory.is_dir():
        raise ReleaseError("candidate must be a real directory")
    content = {}
    for member in directory.iterdir():
        if member.is_symlink() or not member.is_file():
            raise ReleaseError("candidate contains links, directories or special files")
        safe_member_name(member.name)
        content[member.name] = member.read_bytes()
    verify_content(content, receipt, version)
    # Only our private staging directory is removed; the previous output survives
    # every validation/build failure until one complete archive is atomically ready.
    with tempfile.TemporaryDirectory(prefix=".ilium-package-", dir=output.parent) as stage:
        staged = Path(stage) / output.name
        write_archive(staged, target, content)
        verify_content(read_archive(staged, target, receipt), receipt, version)
        with staged.open("rb") as stream:
            os.fsync(stream.fileno())
        os.replace(staged, output)
    emit({"type": "artifact", "path": str(output), "sha256": digest(output.read_bytes()), "bytes": output.stat().st_size})
    emit({"type": "result", "command": "package", "state": "passed", "archive": str(output), "version": version, "members": sorted(content)})


def verify_package(arguments):
    target = selected_target(arguments.manifest, arguments.target)
    version = workspace_version(arguments.workspace, arguments.tag)
    receipt = audit_receipt(arguments.audit_report, target, version, arguments.tag)
    if arguments.archive.name != target["archive"] or arguments.archive.is_symlink():
        raise ReleaseError("archive name differs from approved target or is a link")
    validate_checksums(arguments.checksums, load_targets(arguments.manifest), arguments.archive)
    content = read_archive(arguments.archive, target, receipt)
    verify_content(content, receipt, version)
    emit({"type": "result", "command": "verify-package", "state": "passed", "archive": str(arguments.archive.resolve()), "sha256": digest(arguments.archive.read_bytes()), "version": version, "members": sorted(content), "native_execution": "receipt-bound; no archive bytes executed"})


POSIX_TABLE_START = "# BEGIN GENERATED POSIX TARGETS"
POSIX_TABLE_END = "# END GENERATED POSIX TARGETS"


def posix_table(targets):
    """Only manifest records define installable targets; aliases normalize uname."""
    archive_names = " ".join(target["archive"] for target in targets)
    lines = [POSIX_TABLE_START, f"checksum_archives='{archive_names}'", 'case "$kernel/$architecture" in']
    kernels = {"linux": "Linux", "macos": "Darwin"}
    for target in targets:
        if target["os"] not in kernels:
            continue
        lines.append(f'    {kernels[target["os"]]}/{target["arch"]}) archive_name={target["archive"]}; target_os={target["os"]}; target={target["rust_target"]} ;;')
    supported = ", ".join(f'{kernels[row["os"]]}/{row["arch"]}' for row in targets if row["os"] in kernels)
    lines.extend([f'    *) fail "Unsupported target; supported: {supported}" ;;', "esac", POSIX_TABLE_END])
    return "\n".join(lines)


def generate_posix_table(arguments):
    targets = load_targets(arguments.manifest)
    original = arguments.installer.read_text(encoding="utf-8")
    if original.count(POSIX_TABLE_START) != 1 or original.count(POSIX_TABLE_END) != 1:
        raise ReleaseError("installer must contain exactly one generated target block")
    before, remainder = original.split(POSIX_TABLE_START)
    _, after = remainder.split(POSIX_TABLE_END)
    updated = before + posix_table(targets) + after
    if arguments.check:
        if updated != original:
            raise ReleaseError("generated POSIX target table differs from targets.toml")
    else:
        arguments.output.write_text(updated, encoding="utf-8")
    emit({"type": "result", "command": "generate-posix-table", "state": "passed", "installer": str(arguments.installer.resolve()), "output": str(arguments.output.resolve()) if arguments.output else None, "sha256": digest(updated.encode()), "targets": sum(row["os"] != "windows" for row in targets)})


def build_parser():
    parser = JsonArgumentParser(description=__doc__, allow_abbrev=False)
    subcommands = parser.add_subparsers(dest="command", required=True)
    targets_parser = subcommands.add_parser("targets", help="validate the five-target manifest", allow_abbrev=False)
    targets_parser.add_argument("--manifest", type=Path, required=True, help="explicit path to targets.toml")
    posix_parser = subcommands.add_parser("generate-posix-table", allow_abbrev=False)
    posix_parser.add_argument("--manifest", type=Path, required=True)
    posix_parser.add_argument("--installer", type=Path, required=True)
    posix_mode = posix_parser.add_mutually_exclusive_group(required=True)
    posix_mode.add_argument("--check", action="store_true")
    posix_mode.add_argument("--output", type=Path, help="explicit generated output path")
    for command in ("package", "verify-package"):
        command_parser = subcommands.add_parser(command, allow_abbrev=False)
        for flag in ("manifest", "workspace", "audit-report"):
            command_parser.add_argument("--" + flag, type=Path, required=True)
        command_parser.add_argument("--target", required=True, help="approved Rust target triple")
        command_parser.add_argument("--tag", required=True, help="exact v-prefixed workspace version")
        if command == "package":
            command_parser.add_argument("--directory", type=Path, required=True)
            command_parser.add_argument("--output", type=Path, required=True)
        else:
            command_parser.add_argument("--archive", type=Path, required=True)
            command_parser.add_argument("--checksums", type=Path, required=True)
    return parser


def main(argv=None):
    arguments = None
    try:
        arguments = build_parser().parse_args(argv)
        if arguments.command == "targets":
            targets = load_targets(arguments.manifest)
            emit({"type": "result", "command": "targets", "manifest": str(arguments.manifest.resolve()), "targets": targets})
        elif arguments.command == "package":
            package(arguments)
        elif arguments.command == "generate-posix-table":
            generate_posix_table(arguments)
        else:
            verify_package(arguments)
        return 0
    except (ReleaseError, OSError, UnicodeError, tomllib.TOMLDecodeError, json.JSONDecodeError, tarfile.TarError, zipfile.BadZipFile, EOFError, AttributeError, TypeError, KeyError) as error:
        emit({"type": "error", "command": arguments.command if arguments else None, "error": str(error)})
        return 2


if __name__ == "__main__":
    sys.exit(main())
