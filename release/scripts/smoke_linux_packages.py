#!/usr/bin/env python3
"""Inspect, install and run the Linux packages built by build_linux_packages.py.

`inspect` never executes package bytes: it inspects deb, rpm, AppImage and Snap
against the build receipt, including an architecture the machine cannot run.
Flatpak installed bytes are checked by `host`; explicit unsupported formats fail. `containers` installs the deb, rpm
and AppImage packages in systemd-nspawn containers from the same Docker distribution images.
`host` installs on the running machine, which must be disposable (a GitHub-hosted
runner or a virtual machine): Snap needs snapd and root, Flatpak a real sandbox,
and the AppImage a real FUSE mount. Every install runs the same lifecycle test
(`release/packaging/linux/lifecycle.sh`) as an unprivileged user. Stdout is JSONL.
"""
from __future__ import annotations

import argparse  # Carry bound installed animation acceptance inputs.
import configparser  # Parse installed Flatpak metadata.
import json  # Retain installed readback evidence.
import re  # Validate receipt and revision identities.
import signal  # Interrupt only the retained mount child.
import stat  # Require a real FUSE character device.
import time  # Bound mount readiness and teardown.

import io
import os
from pathlib import Path
import platform
import shlex
import shutil
import struct
import subprocess
import sys
import tarfile
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
import build_linux_packages as packages
import release_tool
import smoke_installed_animation as animation_gate
import linux_container_fixture as container_fixture  # Provision and verify only owned distribution fixtures.

ROOT = Path(__file__).resolve().parents[2]
LIFECYCLE = ROOT / 'release/packaging/linux/lifecycle.sh'
DEB_IMAGES = ('ubuntu:22.04', 'ubuntu:24.04', 'debian:12')
RPM_IMAGES = ('fedora:41', 'opensuse/leap:15.6')
APPIMAGE_IMAGE = 'ubuntu:24.04'
UNPRIVILEGED = 'setpriv --reuid=65534 --regid=65534 --clear-groups'


def require(condition, message):
    if not condition:
        raise release_tool.ReleaseError(message)


def emit(kind, **values):
    release_tool.emit({'type': kind, **values})


mode_formats = {  # Declare exactly which formats each existing mode can prove.
    'inspect': ('deb', 'rpm', 'appimage', 'snap'),  # Flatpak is verified after private deployment.
    'containers': ('deb', 'rpm', 'appimage'),  # Keep the existing distribution-container lanes.
    'host': ('snap', 'appimage', 'flatpak', 'deb'),  # RPM remains a distribution-container gate.
}  # Explicit unsupported requests fail instead of being filtered.
smoke_errors = (ValueError, OSError, KeyError, subprocess.SubprocessError, tarfile.TarError, struct.error, configparser.Error)  # Preserve CLI error conversion.


def selected_formats(arguments):  # Validate the entire request before any installation.
    requested = arguments.formats.split(',')  # Preserve the comma-separated public option.
    require(requested and all(requested), 'the format list must not be empty')  # Reject empty coverage.
    require(len(requested) == len(set(requested)), 'duplicate package format requested')  # Keep one result per format.
    require(set(requested) <= set(mode_formats[arguments.command]), 'unsupported %s formats: %s' % (arguments.command, requested))  # Fail unsupported coverage.
    return requested  # Retain the caller's requested order.


def native_architecture(architecture):  # Runtime checks require a native Linux execution environment.
    aliases = {'x86_64': {'x86_64', 'AMD64'}, 'aarch64': {'aarch64', 'arm64'}}  # Preserve supplied architecture aliases.
    require(platform.system() == 'Linux' and platform.machine() in aliases[architecture], 'native Linux %s is required' % architecture)  # Exclude cross-architecture qualification.
    return aliases[architecture]  # Also validate a possibly remote Docker daemon.


def load_receipt(directory, architecture):  # Validate the existing receipt API before trusting its paths.
    directory = Path(directory)  # Accept existing Path or string callers.
    receipt = release_tool.read_json(directory / packages.receipt_name(architecture))  # Keep the receipt filename contract.
    require(isinstance(receipt, dict) and receipt.get('schema') == 1 and receipt.get('arch') == architecture, 'package receipt is for another architecture')  # Bind receipt schema and architecture.
    version = receipt.get('version')  # Use the package version, not the checkout version.
    require(isinstance(version, str) and packages.VERSION_PATTERN.fullmatch(version), 'invalid package version')  # Prevent malformed runtime expectations.
    require(receipt.get('tag') == 'v' + version, 'package receipt tag differs from version')  # Preserve release identity.
    require(re.fullmatch('[0-9a-f]{64}', str(receipt.get('source_archive_sha256', ''))), 'missing source archive identity')  # Require the existing provenance field.
    expected = receipt.get('package_files')  # This map is independently bound to the native audit by aggregate.
    require(isinstance(expected, dict) and {'ilium', 'ilium-server', 'ilium-animation-helper', *release_tool.APPROVED_PACKAGES, 'VERSION', 'THIRD-PARTY.txt'} <= set(expected), 'incomplete audited member inventory')  # Reject vacuous hash proofs.
    require(all(isinstance(name, str) and packages.MEMBER_PATTERN.fullmatch(name) and isinstance(digest, str) and re.fullmatch('[0-9a-f]{64}', digest) for name, digest in expected.items()), 'invalid audited member inventory')  # Admit only the supplied flat member grammar.
    require(all(expected[name] == digest for name, digest in release_tool.APPROVED_PACKAGES.items()), 'official animation package differs from compiled release identity')
    artifacts = receipt.get('packages')  # Partial-format builds remain supported.
    require(isinstance(artifacts, dict) and artifacts and set(artifacts) <= set(packages.package_names(architecture)), 'invalid package artifact inventory')  # Never interpret arbitrary receipt paths.
    for name, digest in artifacts.items():  # Recheck every supplied package artifact.
        path = directory / name  # Use only the validated basename.
        require(isinstance(digest, str) and re.fullmatch('[0-9a-f]{64}', digest), 'invalid package digest: ' + name)  # Reject malformed hashes.
        require(path.is_file() and not path.is_symlink() and packages.sha(path) == digest, 'package bytes differ from the receipt: ' + name)  # Bind executed packages to receipt bytes.
    return receipt  # Keep the original return shape.


def requested_package(arguments, receipt, package_format):  # Bind each requested operation to a listed artifact.
    name = packages.package_name(arguments.arch, package_format)  # Preserve stable package names.
    require(name in receipt['packages'], 'requested package is absent from the receipt: ' + name)  # Reject incomplete requested builds.
    return arguments.packages.resolve() / name  # Commands receive absolute artifact paths.


def plain_directory(path):  # Reject symlinked payload ancestors without rejecting a resolved mount base.
    path = Path(path)  # Normalize callers without following a payload symlink.
    for item in (path, *path.parents):  # Inspect every ancestor used by the readback.
        require(item.is_dir() and not item.is_symlink(), 'payload directory is missing or symlinked: ' + str(item))  # Never hash through an adopted directory link.


def verify_core(root, receipt):  # Verify exactly the members materialized beside the installed client.
    root = Path(root)  # Support deployed, mounted, extracted and cached roots.
    plain_directory(root)  # Fail before traversing an unexpected directory link.
    expected = {name: digest for name, digest in receipt['package_files'].items() if name != 'THIRD-PARTY.txt'}  # Notices have a separate canonical location.
    require(set(item.name for item in root.iterdir()) == set(expected), 'installed payload inventory differs: ' + str(root))  # Reject extras, including misplaced duplicate notices.
    actual = {}  # Retain the exact readback inventory.
    for name, digest in sorted(expected.items()):  # Hash every audited runtime member.
        path = root / name  # Receipt names have already been validated at the CLI boundary.
        require(path.is_file() and not path.is_symlink(), 'installed payload is not a regular file: ' + str(path))  # Reject links and special entries.
        actual[name] = packages.sha(path)  # Read the installed bytes, not manager metadata.
        require(actual[name] == digest, 'installed payload hash differs: ' + str(path))  # Wrong bytes must fail before behavior qualifies.
    return actual  # Callers can retain exact installed evidence.


def verify_payload(package_format, tree, receipt):  # Share installed and offline audited-member verification.
    root = payload_root(package_format, tree)  # Preserve the existing FHS/Snap layout contract.
    actual = verify_core(root, receipt)  # Enforce the exact flat runtime inventory.
    notice = notices_path(package_format, tree)  # Locate notices independently of the runtime directory.
    plain_directory(notice.parent)  # Reject redirected documentation paths.
    require(notice.is_file() and not notice.is_symlink(), 'installed notices are missing or symlinked: ' + str(notice))  # Require the audited notice itself.
    actual['THIRD-PARTY.txt'] = packages.sha(notice)  # Read the installed notice bytes.
    require(actual == receipt['package_files'], 'installed notices or payload differ from the audit map')  # Compare the complete audited inventory.
    return actual  # Host logs retain these readback hashes.


def verify_launchers(tree, bin_directory):  # Bind launcher behavior to the audited installed executables.
    plain_directory(Path(tree) / bin_directory)  # Reject a redirected launcher directory.
    names = packages.EXECUTABLE_MEMBERS if bin_directory == 'bin' else packages.PUBLIC_EXECUTABLE_MEMBERS
    for name in names:  # Flatpak also exposes its helper inside the sandbox.
        path = Path(tree) / bin_directory / name  # Use the format's documented installation prefix.
        require(path.is_symlink() and os.readlink(path) == '../lib/ilium/' + name, 'installed launcher differs: ' + str(path))  # Reject PATH shadows and redirected links.


def fhs_paths(package_format):  # Enumerate package-owned locations, excluding shared parent directories.
    app_id = packages.APP_ID  # Use the builder's exact desktop identity.
    return [  # These paths are produced by install_tree in the supplied builder.
        '/usr/lib/ilium', '/usr/bin/ilium', '/usr/bin/ilium-server',  # Include the entire runtime root and both launchers.
        '/usr/share/doc/ilium/THIRD-PARTY.txt',  # Include the separately installed audited notices.
        '/usr/share/licenses/ilium/LICENSE' if package_format == 'rpm' else '/usr/share/doc/ilium/copyright',  # Preserve the format's license location.
        '/usr/share/applications/' + app_id + '.desktop',  # Retain desktop-integration removal coverage.
        '/usr/share/metainfo/' + app_id + '.metainfo.xml',  # Retain package metainfo coverage.
        '/usr/share/icons/hicolor/256x256/apps/' + app_id + '.png',  # Include the installed raster icon.
        '/usr/share/icons/hicolor/scalable/apps/' + app_id + '.svg',  # Include the installed vector icon.
    ]  # No shared directory is removed by the smoke itself.


def absence_script(paths):  # Generate a read-only check that treats dangling links as leftovers.
    return '\n'.join('test ! -e %s # Refuse an existing owned path.\ntest ! -L %s # Refuse a dangling owned link.' % (shlex.quote(str(path)), shlex.quote(str(path))) for path in paths) + '\n'  # Keep both tests outside an errexit-exempt AND list.


def inventory_script(package_format):  # Successful complete inventory reads distinguish absence from manager failure.
    query = "dpkg-query -W -f='${Package}\\n'" if package_format == 'deb' else "rpm -qa --qf '%{NAME}\\n'"  # Use documented machine-readable package names.
    return '\n'.join([  # Run the query separately so a pipeline cannot hide its exit status.
        'set -eu # Fail each readback operation.',  # Apply failure semantics in a separate shell.
        'listing=$(mktemp) # Allocate a run-owned inventory file.',  # Never reuse stale package-query output.
        'trap \'rm -f "$listing"\' EXIT # Remove only that inventory file.',  # Retain the readback exit status.
        query + ' > "$listing" # Require a successful manager inventory.',  # A broken database cannot mean absent.
        "awk '$0 == \"ilium\" {found=1} END {exit found ? 1 : 0}' \"$listing\" # Refuse an Ilium registration.",  # Match the exact package name.
        absence_script(fhs_paths(package_format)),  # Check every known installation location.
    ]) + '\n'  # Preserve executable shell text with a final newline.


def write_smoke_files(directory, receipt):  # Supply the same behavioral checks to host and container lanes.
    directory = Path(directory)  # The caller owns this fresh directory.
    core = {name: digest for name, digest in receipt['package_files'].items() if name != 'THIRD-PARTY.txt'}  # Match AppRun's materialized members.
    files = {  # All generated files are local test inputs, never release artifacts.
        'expected.sha256': expected_hashes(receipt),  # Preserve the existing deb/rpm checksum input.
        'core.sha256': ''.join('%s  %s\n' % (digest, name) for name, digest in sorted(core.items())),  # Check materialized files relative to their root.
        'core.names': ''.join(name + '\n' for name in sorted(core)),  # Compare the complete runtime namespace.
        'identity': receipt['version'] + '-' + receipt['package_files']['ilium'][:16] + '\n',  # Match the supplied AppRun cache key.
        'absent-deb.sh': inventory_script('deb'),  # Require manager and path absence together.
        'absent-rpm.sh': inventory_script('rpm'),  # Retain the native RPM database check.
        'check-core.sh': '\n'.join([  # Use shell tools already present in the existing container families.
            '#!/bin/sh',  # Run this helper in its own failure-sensitive shell.
            'set -eu # Fail every inventory or checksum operation.',  # Do not rely on the caller's errexit context.
            'here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd) # Locate immutable smoke inputs.',  # Avoid an ambient working-directory dependency.
            'root=$1 # Receive the exact installed payload root.',  # Every caller passes a quoted path.
            'parent=$root # Check every payload ancestor.',  # Prevent directory-link substitution.
            'while :; do # Walk toward the filesystem root.',  # Stop explicitly at slash.
            '    test -d "$parent" # Require each payload ancestor.',  # Missing directories must independently fail.
            '    test ! -L "$parent" # Reject redirected ancestors.',  # Keep this check outside an errexit-exempt AND list.
            '    test "$parent" != / || break # Finish at the real root.',  # Avoid a repeated dirname slash.
            '    parent=$(dirname -- "$parent") # Move to the next ancestor.',  # Operate on a path, never file contents.
            'done # Complete directory ownership checks.',  # Continue only after all ancestors passed.
            'listing=$(mktemp) # Allocate private inventory scratch.',  # The application cannot supply the file contents.
            'trap \'rm -f "$listing"\' EXIT # Remove only the owned scratch file.',  # Preserve command failure.
            'find "$root" -mindepth 1 -maxdepth 1 -printf "%f\\n" > "$listing" # Read the actual namespace.',  # Keep find failure separate from sort.
            'LC_ALL=C sort -o "$listing" "$listing" # Normalize deterministic member ordering.',  # Match Python's ASCII member ordering.
            'cmp "$here/core.names" "$listing" # Reject extra or missing members.',  # Include misplaced notices in rejection.
            'find "$root" -mindepth 1 -maxdepth 1 ! -type f -print > "$listing" # Detect nonregular members.',  # Symlinks are not followed by find.
            'test ! -s "$listing" # Refuse links, subdirectories and special entries.',  # A matching name alone is insufficient.
            '(cd "$root" && sha256sum -c "$here/core.sha256") # Read every installed runtime byte.',  # Use the receipt-bound expected hashes.
        ]) + '\n',  # Finish the complete helper source.
    }  # The lifecycle implementation itself remains untouched.
    for name, content in files.items():  # Materialize the complete check inputs.
        path = directory / name  # Keep all generated helpers inside the owned scope.
        path.write_text(content, encoding='utf-8')  # Retain exact LF source and data.
        path.chmod(0o644)  # Helpers are invoked through sh and need only read access.
    shutil.copyfile(LIFECYCLE, directory / 'lifecycle.sh')  # Preserve the supplied lifecycle and readiness retry verbatim.
    (directory / 'lifecycle.sh').chmod(0o644)  # Permit the unprivileged container user to read it.


def appimage_wrapper(image, checks, version):  # Check the actual cache used by every successful image invocation.
    return '\n'.join([  # Preserve lifecycle stdout while sending hashes to diagnostics.
        '#!/bin/sh',  # The wrapper remains compatible with lifecycle.sh's command array.
        'set -eu # Preserve any failed application invocation.',  # Do not mask a failed lifecycle command.
        shlex.quote(str(image)) + ' "$@" # Exercise the actual AppImage launcher.',  # The host or container environment chooses its declared execution mode.
        'identity=$(cat %s) # Read the receipt-bound cache identity.' % shlex.quote(str(Path(checks) / 'identity')),  # Use exactly the builder's key.
        'target=$XDG_DATA_HOME/ilium/appimage/$identity # Locate this invocation\'s materialized pair.',  # Lifecycle may change XDG_DATA_HOME between runs.
        'sh %s "$target" >&2 # Verify actual materialized runtime bytes.' % shlex.quote(str(Path(checks) / 'check-core.sh')),  # Keep ls stdout usable by the lifecycle contract.
        'test "$("$target/ilium-server" --version)" = %s # Check the installed sibling server.' % shlex.quote('ilium-server ' + version),  # Preserve exact server-version expectations.
        'test "$("$target/ilium-animation-helper" --version)" = %s # Check the installed animation helper.' % shlex.quote(release_tool.helper_version_record(version)),
    ]) + '\n'  # Return complete executable wrapper source.


def expected_layout(package_format, receipt):  # Enumerate the complete builder-owned data payload, including integrations.
    root = 'lib/ilium' if package_format in ('snap', 'flatpak') else 'usr/lib/ilium'  # Use the supplied per-format directory contract.
    entries = {root + '/' + name: None for name in receipt['package_files'] if name != 'THIRD-PARTY.txt'}  # Admit exactly the audited runtime members.
    if package_format in ('snap', 'flatpak'):  # These formats place documentation beside lib rather than beneath usr.
        entries.update({'share/doc/ilium/THIRD-PARTY.txt': None, 'share/doc/ilium/LICENSE': None})  # Include audited notices and the package's own license.
    else:  # FHS integrations have an explicit finite builder inventory.
        entries.update({path.lstrip('/'): None for path in fhs_paths(package_format) if path != '/usr/lib/ilium'})  # Include desktop metadata, icons, license and launchers.
    if package_format != 'snap':  # Snap uses declared manager commands rather than in-payload bin links.
        prefix = 'bin/' if package_format == 'flatpak' else 'usr/bin/'  # Flatpak supplies a files/ tree to this verifier.
        names = packages.EXECUTABLE_MEMBERS if package_format == 'flatpak' else packages.PUBLIC_EXECUTABLE_MEMBERS
        entries.update({prefix + name: '../lib/ilium/' + name for name in names})  # Preserve the format's exact launcher link targets.
    if package_format == 'snap':  # Reject unrequested hooks or other executable Snap additions.
        entries['meta/snap.yaml'] = None  # The supplied builder writes exactly one metadata file.
    if package_format == 'flatpak':  # Flatpak adds this lock file while deploying an application.
        entries['.ref'] = None  # The manager-created file must be regular and empty.
    if package_format == 'appimage':  # Enumerate the supplied AppDir-specific integration members.
        entries.update({'AppRun': None, packages.APP_ID + '.desktop': None, packages.APP_ID + '.png': None, '.DirIcon': packages.APP_ID + '.png'})  # No extra executable payload is permitted.
    return entries  # None denotes a regular file; a string denotes an exact symlink target.


def layout_reference(package_format, tree, receipt):  # Reject arbitrary package payload paths before native installation.
    tree = Path(tree)  # This tree is either an owned extraction or a resolved read-only deployment.
    expected = expected_layout(package_format, receipt)  # Derive the complete supplied builder's data namespace.
    parents = {parent.as_posix() for name in expected for parent in Path(name).parents if parent.as_posix() != '.'}  # Keep package paths portable when inspected on Windows.
    actual = {}  # Retain ancillary hashes and exact links as package-bound evidence.
    for path in tree.rglob('*'):  # Walk the owned tree without accepting unknown leaf paths.
        name = path.relative_to(tree).as_posix()  # Use canonical relative package member names.
        if path.is_dir() and not path.is_symlink():  # Directories may only be structural parents.
            require(name in parents, 'unexpected package directory: ' + name)  # Reject extra hook or nested-payload directories.
            continue  # Leaf validation follows separately.
        require(name in expected, 'unexpected package payload: ' + name)  # Reject arbitrary files outside lib/ilium too.
        target = expected[name]  # Distinguish declared launchers from regular files.
        if target is not None:  # Only explicitly enumerated symlinks are allowed.
            require(path.is_symlink() and os.readlink(path) == target, 'unexpected package link: ' + name)  # Preserve exact integration link targets.
            actual[name] = {'link': target}  # Retain the package's declared link identity.
            continue  # Never hash through a package symlink.
        require(path.is_file() and not path.is_symlink(), 'nonregular package payload: ' + name)  # Reject devices, pipes and undeclared links.
        require(package_format != 'flatpak' or name != '.ref' or path.stat().st_size == 0, 'unexpected Flatpak lock-file content')  # Admit only the documented empty manager marker.
        actual[name] = {'sha256': packages.sha(path)}  # Bind ancillary bytes to the exact package artifact too.
    require(set(actual) == set(expected), 'package data inventory is incomplete')  # No expected integration or license may disappear.
    return actual  # Installed readback can compare every package-owned leaf against this reference.


def package_reference(package_format, path, receipt, architecture, destination):  # Perform strict nonexecuting pre-install admission.
    unpack(package_format, path, destination)  # Keep the existing unpacker behavior and architecture independence.
    verify_payload(package_format, destination, receipt)  # Bind the core and notices to the audited receipt inventory.
    reference = layout_reference(package_format, destination, receipt)  # Reject arbitrary data paths or Snap hooks before installation.
    if package_format == 'deb':  # The supplied deb builder has no maintainer scripts.
        data, position, members = Path(path).read_bytes(), 8, {}  # Read the complete known ar control structure.
        while position < len(data):  # Parse every ar member before accepting installation metadata.
            header = data[position:position + 60]  # Retain the original fixed-width ar layout.
            name, size = header[:16].decode().strip().rstrip('/'), int(header[48:58])  # Decode the deterministic builder member.
            require(name not in members, 'duplicate deb archive member')  # Do not let a later control archive replace the inspected one.
            members[name] = data[position + 60:position + 60 + size]  # Retain the exact compressed member bytes.
            position += 60 + size + size % 2  # Advance over ar's even-byte padding.
        require(list(members) == ['debian-binary', 'control.tar.xz', 'data.tar.xz'], 'unexpected deb control structure')  # Reject undeclared ar payloads.
        with tarfile.open(fileobj=io.BytesIO(members['control.tar.xz']), mode='r:xz') as archive:  # Inspect control entries without extracting or executing them.
            controls = archive.getmembers()  # The supplied builder emits only the root, control and md5sums.
            require([item.name for item in controls] == ['.', './control', './md5sums'] and controls[0].isdir() and all(item.isreg() for item in controls[1:]), 'unexpected deb maintainer/control payload')  # Never admit unaudited maintainer scripts.
            control = archive.extractfile(controls[1]).read().decode('utf-8').splitlines()  # Inspect the exact package identity before installation.
            for key, value in (('Package', 'ilium'), ('Version', packages.deb_version(receipt['version'])), ('Architecture', packages.ARCHITECTURES[architecture]['deb'])):  # Bind the package-manager target to the receipt.
                require([line for line in control if line.startswith(key + ':')] == [key + ': ' + value], 'deb control identity differs: ' + key)  # Reject duplicate or substituted identity fields.
    if package_format == 'rpm':  # The supplied RPM contains no install scripts or triggers.
        identity = subprocess.run(['rpm', '-qp', '--qf', '%{NAME}\\n%{VERSION}\\n%{RELEASE}\\n%{ARCH}\\n', str(path)], capture_output=True, text=True, timeout=60)  # Read package identity without executing it.
        require(identity.returncode == 0 and identity.stdout.splitlines() == ['ilium', receipt['version'].replace('-', '~', 1), '1', packages.ARCHITECTURES[architecture]['rpm']], 'RPM package identity differs')  # Never install a different manager target.
        for option in ('--scripts', '--triggers', '--filetriggers'):  # Query metadata without executing package code.
            result = subprocess.run(['rpm', '-qp', option, str(path)], capture_output=True, text=True, timeout=60)  # Use the native RPM inspection contract.
            require(result.returncode == 0 and not result.stdout.strip(), 'unexpected or unverified RPM script metadata: ' + option)  # Fail absent tools and unexpected package code.
    if package_format == 'snap':  # Reject a changed command definition before snapd can execute it.
        require((Path(destination) / 'meta/snap.yaml').read_text(encoding='utf-8') == packages.render_snap_yaml(receipt['version'], architecture), 'Snap metadata differs from the supplied builder')  # Preserve both architecture and exact client/server commands.
    if package_format == 'appimage':  # AppRun is an explicitly supplied packaging shim rather than an audited binary.
        expected = packages.render_template(packages.APPRUN, {'VERSION': receipt['version'], 'IDENT': receipt['package_files']['ilium'][:16]})  # Reconstruct its exact existing source-defined bytes.
        require((Path(destination) / 'AppRun').read_text(encoding='utf-8') == expected, 'AppImage AppRun differs from the supplied builder')  # Do not permit arbitrary replacement launcher code.
    return reference  # Every admitted package leaf is now accounted for.


def reference_script(reference):  # Generate complete installed readback of core, licenses and integration bytes.
    lines = ['set -eu # Require every package-owned leaf readback.', 'root=${1:-/} # Select the actual deployment prefix.']  # Each check executes in its own failure-sensitive script.
    for name, record in sorted(reference.items()):  # Retain the complete strict package inventory.
        path = Path(name)  # Retain a known-safe relative package member name.
        for parent in reversed(path.parents):  # Reject redirected installed directory ancestors.
            quoted = '"$root/' + parent.as_posix() + '"'  # Expand only the caller's separately quoted root.
            lines += ['test -d ' + quoted + ' # Require the installed parent.', 'test ! -L ' + quoted + ' # Reject a redirected parent.']  # Neither condition is hidden in an AND list.
        quoted = '"$root/' + path.as_posix() + '"'  # Known relative paths contain no shell metacharacters.
        if 'link' in record:  # Verify declared launcher links without following them.
            lines += ['test -L ' + quoted + ' # Require the declared launcher link.', 'test "$(readlink ' + quoted + ')" = ' + shlex.quote(record['link']) + ' # Bind its exact target.']  # Reject a substituted executable or link.
            continue  # Regular file hashing must not follow a declared symlink.
        lines += ['test -f ' + quoted + ' # Require a regular installed file.', 'test ! -L ' + quoted + ' # Refuse an undeclared file link.', 'printf %s ' + shlex.quote(record['sha256'] + '  ' + path.as_posix() + '\n') + ' | (cd "$root" && sha256sum -c -) # Verify package-bound installed bytes.']  # Serialize package members with POSIX separators on every inspector platform.
    return '\n'.join(lines) + '\n'  # Return the complete reusable host/container verifier.


def squashfs_offset(path):
    """Size of the ELF runtime an AppImage starts with: the end of its section header table."""
    with Path(path).open('rb') as source:
        header = source.read(64)
    require(header[:4] == b'\x7fELF' and header[4] == 2, 'AppImage does not start with an ELF64 runtime')
    section_offset = struct.unpack_from('<Q', header, 0x28)[0]
    section_size, section_count = struct.unpack_from('<HH', header, 0x3A)
    return section_offset + section_size * section_count


def unpack(package_format, path, destination):
    destination = Path(destination)
    if package_format == 'deb':
        data = Path(path).read_bytes()
        require(data[:8] == b'!<arch>\n', 'deb is not an ar archive')
        position, members = 8, {}
        while position < len(data):
            header = data[position:position + 60]
            name, size = header[:16].decode().strip().rstrip('/'), int(header[48:58])
            members[name] = data[position + 60:position + 60 + size]
            position += 60 + size + size % 2
        require(list(members)[:1] == ['debian-binary'] and members['debian-binary'] == b'2.0\n', 'deb member order is wrong')
        with tarfile.open(fileobj=io.BytesIO(members['data.tar.xz']), mode='r:xz') as archive:
            archive.extractall(destination, filter='tar')
    elif package_format == 'rpm':
        require(shutil.which('rpm2cpio') and shutil.which('cpio'), 'rpm2cpio and cpio are required to inspect an rpm')
        destination.mkdir(parents=True)
        converted = subprocess.run(['rpm2cpio', str(path)], capture_output=True, check=True).stdout
        subprocess.run(['cpio', '-idm', '--quiet'], input=converted, cwd=destination, check=True, capture_output=True)
    elif package_format in ('appimage', 'snap'):
        require(shutil.which('unsquashfs') is not None, 'unsquashfs is required to inspect an AppImage or snap')
        offset = squashfs_offset(path) if package_format == 'appimage' else 0
        subprocess.run(['unsquashfs', '-quiet', '-no-progress', '-o', str(offset), '-d', str(destination), str(path)], check=True, capture_output=True)
    else:
        raise release_tool.ReleaseError('no offline unpacker for ' + package_format)


def payload_root(package_format, tree):
    tree = Path(tree)
    return tree / {'deb': 'usr/lib/ilium', 'rpm': 'usr/lib/ilium', 'appimage': 'usr/lib/ilium', 'snap': 'lib/ilium', 'flatpak': 'lib/ilium'}[package_format]  # Flatpak callers supply deployment/files.


def notices_path(package_format, tree):
    tree = Path(tree)
    return tree / ('share/doc/ilium' if package_format in ('snap', 'flatpak') else 'usr/share/doc/ilium') / 'THIRD-PARTY.txt'  # Keep each format's canonical notice location.


def expected_hashes(receipt):  # Preserve the directly tested checksum-file API.
    """`sha256sum -c` lines for the deb/rpm install paths of every audited file."""  # Keep notices outside the runtime directory.
    return ''.join('%s  %s\n' % (digest, '/usr/share/doc/ilium/THIRD-PARTY.txt' if name == 'THIRD-PARTY.txt' else '/usr/lib/ilium/' + name) for name, digest in sorted(receipt['package_files'].items()))  # Preserve exact sorted output bytes.


def report(command, package_format, environment, result, extra=None):  # Preserve JSONL result shape and integer failure count.
    state = 'passed' if result.returncode == 0 and (extra or {}).get('removed', True) else 'failed'  # Failed removal can never qualify a successful command.
    values = dict(command=command, format=package_format, environment=environment, state=state, **(extra or {}))  # Retain existing caller-supplied evidence fields.
    if state == 'failed':  # Include bounded diagnostics for every failed result.
        values['error'] = (result.stdout + result.stderr)[-1200:]  # Preserve the existing diagnostic-tail contract.
    emit('result', **values)  # Keep existing stdout consumers compatible.
    return 0 if state == 'passed' else 1  # Removal failure also contributes to the summary failure count.


def inspect_package(package_format, path, receipt, architecture):  # Preserve the directly tested offline API.
    with tempfile.TemporaryDirectory(prefix='ilium-inspect-') as temporary:  # Own all unpacked inspection data.
        tree = Path(temporary) / 'tree'  # Keep extraction separate from the caller's artifacts.
        if 'packages' in receipt:  # Full CLI receipts also require strict whole-package admission.
            package_reference(package_format, path, receipt, architecture, tree)  # Reject undeclared data or install-time code before native execution.
        else:  # Preserve the directly tested minimal audit-map API.
            unpack(package_format, path, tree)  # Keep pure payload fixtures usable without metadata stubs.
        actual = verify_payload(package_format, tree, receipt)  # Reject extra, missing, redirected or changed audited members.
        return {'files': len(actual)}  # Preserve the exact existing result shape.


def inspect(arguments):  # Offline inspection never claims native behavior or unsupported coverage.
    formats = selected_formats(arguments)  # Reject unsupported explicit requests before reading package bytes.
    receipt = load_receipt(arguments.packages, arguments.arch)  # Bind every inspected artifact to the receipt.
    failed = 0  # Count requested-format failures.
    for package_format in formats:  # Emit a result for every applicable requested format.
        name = packages.package_name(arguments.arch, package_format)  # Preserve output package names.
        try:  # Keep one failure from hiding other requested inspection results.
            path = requested_package(arguments, receipt, package_format)  # Reject an unlisted requested artifact.
            detail = inspect_package(package_format, path, receipt, arguments.arch)  # Inspect the complete audited member set.
            emit('result', command='inspect', format=package_format, package=name, arch=arguments.arch, state='passed', execution='none', **detail)  # State the proof's nonexecuting scope.
        except smoke_errors as error:  # Preserve bounded diagnostics for a failed inspector.
            failed += 1  # Fail the requested-format summary.
            emit('result', command='inspect', format=package_format, package=name, state='failed', error=str(error)[:1200])  # Identify the exact failed format.
    return failed  # Preserve the original count-based API.


def managed_script(package_format, prelude, install, remove, body):  # Preserve primary failure while making cleanup authoritative.
    absent = '/smoke/absent-' + package_format + '.sh'  # Reuse the exact before/after inventory gate.
    return '\n'.join([  # Each cleanup command has its own explicit failure handling.
        'set -eu # Fail native acceptance on the first functional error.',  # Run through the existing sh -ec interface.
        'owned=0 # Never remove an installation before ownership preflight.',  # Installation ownership is armed only after absence.
        'cleanup() { # Preserve both primary failure and cleanup outcome.',  # Trap-owned teardown is part of qualification.
        '    prior=$? # Retain the original command status.',  # Successful removal must not erase a failed smoke.
        '    trap - EXIT # Avoid recursively invoking cleanup.',  # Run exactly one trap teardown.
        '    cleanup_failed=0 # Evaluate removal and readback independently.',  # Do not infer absence from a failed remover.
        '    if [ "$owned" = 1 ]; then # Only remove this run\'s attempted install.',  # A failed install may have left partial state.
        '        ' + remove + ' || cleanup_failed=1 # Retain removal failure.',  # Never suppress a failed removal into success.
        '        sh ' + absent + ' || cleanup_failed=1 # Require authoritative absence.',  # A failed query is a failed gate.
        '    fi # Finish ownership-limited cleanup.',  # Existing installations are never adopted.
        '    test "$prior" = 0 && test "$cleanup_failed" = 0 # Return both outcomes.',  # Failed cleanup must fail the container.
        '} # Complete the bounded transaction trap.',  # No broad filesystem deletion is used.
        'trap cleanup EXIT # Clean attempted installations on every exit.',  # Preserve the original lifecycle's independent trap.
        prelude,  # Keep existing distro prerequisite preparation.
        'sh ' + absent + ' # Refuse existing package registrations or paths.',  # Prove a fresh namespace before install.
        'owned=1 # Mark the subsequent package-manager attempt as ours.',  # Retain responsibility for partial installs.
        install,  # Run the existing native package-manager installation.
        body,  # Verify installed bytes, both versions and the real lifecycle.
        remove,  # Demand successful explicit removal on the positive path.
        'sh ' + absent + ' # Confirm package-manager and filesystem absence.',  # Include both launchers, runtime, notices and integrations.
        'owned=0 # The explicit removal and readback have both passed.',  # The EXIT trap no longer repeats removal.
    ]) + '\n'  # Return complete executable shell source.


def installed_body(version):  # Share complete FHS installed behavior across deb and rpm.
    return '\n'.join([  # Keep the existing checksum and unprivileged lifecycle proofs.
        '(cd / && sha256sum -c /smoke/expected.sha256) # Read all audited installed members.',  # Preserve notices outside the runtime root.
        'sh /smoke/check-core.sh /usr/lib/ilium # Require an exact regular-file payload inventory.',  # Reject unexpected members too.
        'test "$(/usr/bin/ilium --version)" = %s # Check the installed client.' % shlex.quote('ilium ' + version),  # Avoid ambient PATH resolution.
        'test "$(/usr/bin/ilium-server --version)" = %s # Check the installed server.' % shlex.quote('ilium-server ' + version),  # Repair the RPM server-version omission.
        'test "$(/usr/lib/ilium/ilium-animation-helper --version)" = %s # Check the installed animation helper.' % shlex.quote(release_tool.helper_version_record(version)),
        'printf "ILIUM_ANIMATION_BEGIN\\n" # Delimit the native installed helper IPC proof.',
        UNPRIVILEGED + ' /usr/bin/ilium release-animation-probe # Exercise both approved packages as the disposable account.',
        'printf "ILIUM_ANIMATION_END\\n" # A failed probe cannot reach this marker under sh -e.',
        'test "$(readlink /usr/bin/ilium)" = ../lib/ilium/ilium # Bind the client launcher.',  # Preserve the builder's exact symlink contract.
        'test "$(readlink /usr/bin/ilium-server)" = ../lib/ilium/ilium-server # Bind the server launcher.',  # Reject redirected sibling launchers.
        UNPRIVILEGED + ' sh /smoke/lifecycle.sh /usr/bin/ilium # Run the unchanged isolated lifecycle.',  # Retain the original readiness retry.
    ]) + '\n'  # Preserve stdout useful to the existing container report.


def deb_script(name, version, *, offline=False):  # Preserve the existing helper signature and all three Debian-family images.
    prelude = 'export DEBIAN_FRONTEND=noninteractive # Disable package-manager prompts.\nrm -f /etc/dpkg/dpkg.cfg.d/excludes # Preserve audited notices.\napt-get update -qq >/dev/null # Refresh this disposable image.'  # Retain existing working setup.
    install = 'apt-get install -y -qq %s >/dev/null # Install the exact deb.' % shlex.quote('/packages/' + name)  # Quote the receipt-bound path.
    if offline:  # Dependency preparation already completed inside the native base image.
        prelude = ": # Use the prepared package database without external networking."  # Keep acceptance in its private network namespace.
        install = install.replace("apt-get install", "apt-get --no-download install")  # Require all dependencies to be locally available.
    body = "dpkg -s ilium | grep -q '^Status: install ok installed' # Verify successful registration.\nsh /smoke/package-deb.sh / # Read back every admitted package-owned leaf.\n" + installed_body(version)  # Retain the existing registration proof.
    return managed_script('deb', prelude, install, 'apt-get remove -y -qq ilium >/dev/null', body)  # Make removal and readback part of success.


def rpm_script(image, name, version, *, offline=False):  # Preserve Fedora and openSUSE package-manager branches.
    path = shlex.quote('/packages/' + name)  # Quote the exact native RPM artifact.
    install = 'zypper --non-interactive --no-gpg-checks install --allow-unsigned-rpm %s >/dev/null' % path if 'suse' in image else 'dnf install -y -q --setopt=tsflags= %s' % path  # Retain existing unsigned-package test semantics.
    remove = 'zypper --non-interactive remove ilium >/dev/null' if 'suse' in image else 'dnf remove -y -q ilium'  # Preserve each manager's known removal contract.
    prelude = ': # openSUSE already provides the lifecycle tools.' if 'suse' in image else 'dnf install -y -q util-linux diffutils >/dev/null # Provide unprivileged execution and exact inventory comparison.'  # Retain existing prerequisite behavior.
    if offline:  # The acceptance boot has only its private loopback network.
        prelude = ': # Prerequisites are already installed in this prepared distribution.'  # Never drop installed verification.
        install = install.replace('zypper ', 'zypper --no-refresh ').replace('dnf ', 'dnf --cacheonly ')  # Use actual native managers and their prepared metadata.
        remove = remove.replace('zypper ', 'zypper --no-refresh ').replace('dnf ', 'dnf --cacheonly ')  # Retain removal and authoritative absence without network refresh.
    return managed_script('rpm', prelude, install, remove, 'rpm -q ilium # Verify registration.\nrpm -V ilium # Preserve RPM integrity checking.\nsh /smoke/package-rpm.sh / # Read back every admitted package-owned leaf.\n' + installed_body(version))  # Preserve RPM verification alongside exact audit hashes.


def appimage_script(name, version, *, offline=False):  # Keep extract-and-run as an explicitly separate container proof.
    return '\n'.join([  # This mode never claims FUSE availability.
        'set -eu # Fail every extraction, behavior and cleanup gate.',  # Preserve the container shell contract.
        'export DEBIAN_FRONTEND=noninteractive # Avoid interactive apt prompts.',  # Keep native dependency installation unattended.
        ': # Use the prepared distribution metadata.' if offline else 'apt-get update -qq >/dev/null # Refresh the disposable image.',  # Preserve the existing Ubuntu lane.
        ': # Runtime dependencies were installed during image preparation.' if offline else 'apt-get install -y -qq libasound2t64 libssl3t64 libstdc++6 >/dev/null # Install existing runtime dependencies.',  # Do not alter the AppImage payload.
        'work=$(mktemp -d /tmp/ilium-appimage.XXXXXX) # Own all extraction and cache data.',  # Never use the developer\'s AppImage cache.
        'trap \'rm -rf "$work"\' EXIT # Limit fallback cleanup to the owned scope.',  # Container failure still propagates through sh -e.
        'chmod 0755 "$work" # Allow the existing unprivileged identity to enter.',  # Avoid root-only temporary-directory permissions.
        'cp %s "$work/ilium.AppImage" # Preserve the input artifact.' % shlex.quote('/packages/' + name),  # Only the private copy is made executable.
        'chmod 0755 "$work/ilium.AppImage" # Permit the pinned runtime to execute.',  # Do not chmod the supplied release file.
        'mkdir "$work/home" "$work/tmp" # Isolate cache, state and runtime directories.',  # Lifecycle inherits an owned TMPDIR.
        'cp /smoke/appimage-client.sh "$work/client.sh" # Use the complete cache-checking wrapper.',  # The immutable wrapper selects this private image.
        'chown -R 65534:65534 "$work" # Give the lifecycle identity its private state.',  # Root never runs the application lifecycle.
        'export HOME="$work/home" XDG_DATA_HOME="$work/home/data" XDG_CONFIG_HOME="$work/home/config" TMPDIR="$work/tmp" ILIUM_SMOKE_BASE="$work/tmp" # Pin disposable application state.',  # No real user data is touched.
        '(cd "$work" && %s ./ilium.AppImage --appimage-extract >/dev/null) # Inspect the runtime\'s extracted AppDir.' % UNPRIVILEGED,  # Exercise the documented extraction path.
        'sh /smoke/package-appimage.sh "$work/squashfs-root" # Compare every extracted leaf to the admitted artifact.',  # Include canonical notices, metadata and launcher links.
        'sh /smoke/check-core.sh "$work/squashfs-root/usr/lib/ilium" # Verify extracted runtime members.',  # Preserve exact audit parity.
        'printf "%s  %s\\n" "$(cat /smoke/notice.sha256)" "$work/squashfs-root/usr/share/doc/ilium/THIRD-PARTY.txt" | sha256sum -c - # Verify extracted audited notices.',  # Notices are not copied into the AppRun cache.
        'export APPIMAGE_EXTRACT_AND_RUN=1 ILIUM_SMOKE_IMAGE="$work/ilium.AppImage" # Select only this container\'s extraction execution mode.',  # The host mode explicitly clears extraction overrides.
        'test "$(%s sh "$work/client.sh" --version)" = %s # Verify client, cached bytes and sibling server.' % (UNPRIVILEGED, shlex.quote('ilium ' + version)),  # The wrapper checks the exact server version too.
        'printf "ILIUM_ANIMATION_BEGIN\\n" # Delimit the installed AppImage cache proof.',
        '%s sh "$work/client.sh" release-animation-probe # Use the real image launcher and cache.' % UNPRIVILEGED,
        'printf "ILIUM_ANIMATION_END\\n" # Preserve the failure-sensitive shell contract.',
        '%s sh /smoke/lifecycle.sh sh "$work/client.sh" # Retain real extract-and-run lifecycle coverage.' % UNPRIVILEGED,  # The wrapper hashes each lifecycle cache before it disappears.
        'rm -rf "$work" # Remove only the owned image, extraction and cache.',  # AppImage has no package-manager registration.
        'test ! -e "$work" # Require verified removal of that scope.',  # Cleanup failure cannot qualify success.
        'test ! -L "$work" # Refuse a dangling replacement scope.',  # Each check must independently trigger errexit.
        'trap - EXIT # The explicit cleanup readback passed.',  # Avoid a redundant removal.
    ]) + '\n'  # Return the complete container smoke script.


def container_animation(arguments, receipt, result, label, package_format, image, log, fixture_record=None):  # Require the owned fixture and real installed-render proof together.
    source_root = Path(arguments.workspace).resolve(strict=True).parent  # Bind the fixture to the selected source workspace.
    fixture_sources = {name: packages.sha(source_root / name) for name in container_fixture.source_files}  # Retain both complete fixture implementations.
    require(result.returncode == 0, "failed container command cannot qualify animation")  # Never accept success-shaped stdout after a failed transaction.
    container_fixture.validate_record(fixture_record, package_format, image, arguments.arch, fixture_sources)  # Reject missing managers, wrong namespaces and incomplete cleanup.
    require(fixture_record["stdout_sha256"] == release_tool.digest(result.stdout.encode("utf-8")) and fixture_record["stderr_sha256"] == release_tool.digest(result.stderr.encode("utf-8")), "container streams differ from retained fixture evidence")  # Bind real output to the completed fixture.
    require(result.stdout.count('ILIUM_ANIMATION_BEGIN\n') == 1 and
            result.stdout.count('ILIUM_ANIMATION_END\n') == 1,
            'container animation proof markers are missing or ambiguous')
    output = result.stdout.split('ILIUM_ANIMATION_BEGIN\n', 1)[1].split(
        'ILIUM_ANIMATION_END\n', 1)[0]
    catalogue, renders = animation_gate.parse_probe_output(output)
    installed = receipt['package_files']
    expected_root = Path('/usr/lib/ilium')
    client_path = Path(catalogue.get('client_path', ''))
    helper_path = Path(catalogue.get('helper_path', ''))
    require((client_path.parent == expected_root if package_format in ('deb', 'rpm') else
             client_path.parent.name == receipt['version'] + '-' + installed['ilium'][:16]
             and client_path.parent.parent.name == 'appimage') and
            client_path.name == 'ilium' and helper_path == client_path.parent / 'ilium-animation-helper' and
            catalogue.get('client_sha256') == installed['ilium'] and
            catalogue.get('helper_sha256') == installed['ilium-animation-helper'],
            'container animation ran a different installed client or helper')
    audit_path = Path(arguments.audit_report).resolve(strict=True)
    source_root = Path(arguments.workspace).resolve(strict=True).parent
    package_name = packages.package_name(arguments.arch, package_format)
    package_path = requested_package(arguments, receipt, package_format)
    require(packages.sha(package_path) == receipt['packages'][package_name],
            'container package bytes changed during native animation smoke')
    source_hashes = {name: packages.sha(source_root / name)
                     for name in animation_gate.SOURCE_FILES}
    native = {'schema': 1, 'state': 'passed', 'publication_allowed': False,
              'scope': 'installed-animation-container', 'tag': receipt['tag'],
              'format': package_format, 'image': image, 'arch': arguments.arch,
              'package': package_name, 'package_sha256': receipt['packages'][package_name],
              'source_archive_sha256': receipt['source_archive_sha256'],
              'native_audit_sha256': packages.sha(audit_path),
              'source_files': source_hashes,
              'installed_files': {name: installed[name]
                                  for name in ('ilium', 'ilium-server', 'ilium-animation-helper',
                                               *release_tool.APPROVED_PACKAGES)},
              'client_path': str(client_path), 'helper_path': str(helper_path),
              'stdout': output, 'stdout_sha256': release_tool.digest(output.encode('utf-8')),
              'catalogue': catalogue, 'renders': renders,
              'fixture': fixture_record}  # Seal the exact owned runtime and cleanup observations.
    path = Path(log) / (label + '-installed-animation.json')
    with path.open('x', encoding='utf-8') as file:
        json.dump(native, file, indent=2, sort_keys=True)
        file.write('\n')
    return {'path': str(path), 'sha256': packages.sha(path),
            'content_sha256': release_tool.digest(json.dumps(
                native, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode('utf-8'))}


def containers(arguments):  # Preserve all existing native distribution jobs with stricter requested coverage.
    formats = selected_formats(arguments)  # Reject unsupported explicit formats before launching containers.
    aliases = native_architecture(arguments.arch)  # Do not label emulation as native acceptance.
    require(Path(container_fixture.docker[0]).is_file(), 'docker is required')  # Missing tooling must fail the requested gate.
    daemon = subprocess.run([*container_fixture.docker, 'info', '--format', '{{.Architecture}}'], capture_output=True, text=True, timeout=60, env={'PATH': '/usr/bin:/bin', 'LANG': 'C', 'LC_ALL': 'C'})  # Query only the explicit local daemon, including this early preflight.
    require(daemon.returncode == 0 and daemon.stdout.strip() in aliases, 'Docker daemon architecture is unverified or differs')  # Exclude a foreign remote daemon.
    receipt = load_receipt(arguments.packages, arguments.arch)  # Keep exact artifact validation.
    target = release_tool.selected_target(Path(arguments.manifest),
                                          arguments.arch + '-unknown-linux-gnu')
    audit = release_tool.audit_receipt(Path(arguments.audit_report), target,
                                       receipt['version'], receipt['tag'])
    require(audit['files'] == receipt['package_files'],
            'container package inventory differs from native audit')
    for package_format in formats:  # Validate all requested inputs before native mutation.
        requested_package(arguments, receipt, package_format)  # Fail an unlisted requested artifact.
    log = arguments.log.resolve()  # Preserve the caller's diagnostics destination.
    failed = 0  # Retain count-based return semantics.
    source_root = Path(arguments.workspace).resolve(strict=True).parent  # Freeze the complete source authority before execution.
    source_snapshot = {name: packages.sha(source_root / name) for name in (*animation_gate.SOURCE_FILES, *container_fixture.source_files, 'release/scripts/smoke_linux_packages.py')}  # Freeze the harness as well as both new fixture owners.
    audit_snapshot = packages.sha(arguments.audit_report)  # Keep the accepted native audit stable throughout the matrix.
    with tempfile.TemporaryDirectory(prefix='ilium-smoke-') as temporary:  # Own all shared check inputs.
        smoke = Path(temporary)  # Bind the existing read-only /smoke mount.
        smoke.chmod(0o755)  # Permit the container's unprivileged user to read helpers.
        write_smoke_files(smoke, receipt)  # Preserve lifecycle bytes and exact checksum manifests.
        for package_format in formats:  # Reject arbitrary package code and data before launching any container.
            reference = package_reference(package_format, requested_package(arguments, receipt, package_format), receipt, arguments.arch, smoke / ('reference-' + package_format))  # Use a fresh nonexecuting extraction per format.
            (smoke / ('package-' + package_format + '.sh')).write_text(reference_script(reference), encoding='utf-8')  # Supply complete installed ancillary and audited-file checks.
        wrapper = appimage_wrapper('/unused', '/smoke', receipt['version']).replace('/unused', '"${ILIUM_SMOKE_IMAGE:?}"')  # Select only the private image supplied by the container script.
        (smoke / 'appimage-client.sh').write_text(wrapper, encoding='utf-8')  # Materialize complete executable wrapper text.
        (smoke / 'notice.sha256').write_text(receipt['package_files']['THIRD-PARTY.txt'] + '\n', encoding='ascii')  # Retain the independently located notice digest.
        jobs = []  # Preserve the original per-distribution test matrix.
        if 'deb' in formats:  # Enqueue all requested Debian-family environments.
            jobs += [('deb', image, deb_script(packages.package_name(arguments.arch, 'deb'), receipt['version'], offline=True)) for image in DEB_IMAGES]  # Keep Ubuntu 22.04/24.04 and Debian 12.
        if 'rpm' in formats:  # Enqueue both requested RPM-family environments.
            jobs += [('rpm', image, rpm_script(image, packages.package_name(arguments.arch, 'rpm'), receipt['version'], offline=True)) for image in RPM_IMAGES]  # Keep Fedora and openSUSE.
        if 'appimage' in formats:  # Enqueue the existing extraction lane.
            jobs.append(('appimage', APPIMAGE_IMAGE, appimage_script(packages.package_name(arguments.arch, 'appimage'), receipt['version'], offline=True)))  # FUSE remains a separate host requirement.
        for package_format, image, script in jobs:  # Execute and retain every configured native environment result.
            label = package_format + '-' + image.replace('/', '_').replace(':', '_')  # Preserve existing log filenames.
            fixture_record = None  # An unattempted or failed fixture can never qualify.
            try:  # Preserve one terminal result for every requested distribution lane.
                result, fixture_record = container_fixture.run_fixture(image, package_format, arguments.arch, arguments.packages.resolve(), smoke, log, label, script, packages.package_name(arguments.arch, package_format), host_run, {name: source_snapshot[name] for name in container_fixture.source_files})  # Run the complete transaction in an owned systemd container.
            except smoke_errors as error:  # Retain final evidence-write or adapter failures too.
                result = subprocess.CompletedProcess([], 1, '', 'container fixture: ' + str(error))  # Keep stdout JSONL at the reporting boundary.
            command = result.args  # Preserve the actual fixture command in any failed result.
            animation_evidence = None
            if result.returncode == 0:
                try:
                    require(source_snapshot == {name: packages.sha(source_root / name) for name in source_snapshot} and audit_snapshot == packages.sha(arguments.audit_report), "container source or audit changed during execution")  # Reject mixed execution/provenance identities.
                    animation_evidence = container_animation(arguments, receipt, result, label,
                                                             package_format, image, log, fixture_record)  # Validate both real installed packages after complete cleanup.
                except smoke_errors as error:
                    result = subprocess.CompletedProcess(command, 1, result.stdout,
                                                         result.stderr + '\ninstalled animation: ' + str(error))
            package_name = packages.package_name(arguments.arch, package_format)
            source_root = Path(arguments.workspace).resolve(strict=True).parent
            failed += report('containers', package_format, image, result, {
                'arch': arguments.arch, 'tag': receipt['tag'], 'package': package_name,
                'package_sha256': receipt['packages'][package_name],
                'source_archive_sha256': receipt['source_archive_sha256'],
                'native_audit_sha256': packages.sha(arguments.audit_report),
                'source_files': {name: packages.sha(source_root / name)
                                 for name in animation_gate.SOURCE_FILES},
                'execution': 'extract-and-run' if package_format == 'appimage' else 'native-container',
                'animation': animation_evidence,
                'fixture_sha256': container_fixture.content_sha(fixture_record) if fixture_record is not None else None})  # Preserve exact consumed artifact and result identity.
    return failed  # A single failed environment fails qualification.


def sudo():
    return [] if os.geteuid() == 0 else ['sudo', '-n']


def host_run(command, log, label, **options):  # Preserve the existing command adapter while retaining timeout diagnostics.
    command = [str(part) for part in command]  # Keep paths safe as separate argv values.
    timeout = options.pop('timeout', 1800)  # Allow bounded mount/readback callers without duplicate options.
    try:  # Convert command failures into reportable outcomes.
        result = subprocess.run(command, capture_output=True, text=True, timeout=timeout, **options)  # Retain direct child custody through subprocess.run.
    except subprocess.TimeoutExpired as error:  # A timed-out native gate must not disappear from logs.
        stdout = error.stdout.decode(errors='replace') if isinstance(error.stdout, bytes) else (error.stdout or '')  # Preserve available partial output.
        stderr = error.stderr.decode(errors='replace') if isinstance(error.stderr, bytes) else (error.stderr or '')  # Preserve available partial diagnostics.
        result = subprocess.CompletedProcess(command, 124, stdout, stderr + '\ncommand timed out\n')  # Never treat timeout as an absent package.
    except OSError as error:  # Missing executables and launch failures also fail closed.
        result = subprocess.CompletedProcess(command, 127, '', str(error) + '\n')  # Retain the command's failed startup.
    Path(log).mkdir(parents=True, exist_ok=True)  # Preserve the caller's diagnostics directory.
    with (Path(log) / (label + '.log')).open('a', encoding='utf-8') as output:  # Keep every transaction phase, including cleanup.
        output.write('$ %s\n%s%s\nexit_code=%d\n' % (shlex.join(command), result.stdout, result.stderr, result.returncode))  # Retain exact argv and command outcomes.
    return result  # Preserve the original CompletedProcess adapter API.


def checked(command, log, label, **options):  # Convert unsuccessful native commands into gate failures.
    result = host_run(command, log, label, **options)  # Retain diagnostics before interpreting the status.
    require(result.returncode == 0, '%s exited %d: %s' % (shlex.join([str(part) for part in command]), result.returncode, (result.stdout + result.stderr)[-1200:]))  # Do not confuse query failure with absence.
    return result  # Callers parse only successful command output.


def transaction(actions):  # Evaluate ownership, behavior and removal as independent required gates.
    stages = {}  # Record every attempted gate without manufacturing unattempted success.
    errors = []  # Keep primary failure and cleanup failures together.
    owned = False  # Pre-existing installations are never ours to uninstall.
    stage = 'preflight'  # Identify failures before an installation attempt.
    try:  # Preserve cleanup after partial installation or failed behavior.
        actions['preflight']()  # Require authoritative absence and usable prerequisites.
        stages[stage] = 'passed'  # Ownership preflight has completed.
        owned = True  # The following installation attempt and its partial state are ours.
        for stage in ('install', 'verify'):  # Keep installation separate from installed acceptance.
            actions[stage]()  # Perform the actual manager or AppImage operation.
            stages[stage] = 'passed'  # Record only completed successful gates.
    except smoke_errors as error:  # Retain a functional failure without bypassing cleanup.
        stages[stage] = 'failed'  # Identify the failing gate.
        errors.append(stage + ': ' + str(error))  # Preserve the primary diagnosis.
    finally:  # Never replace primary failure with a successful uninstall.
        if owned:  # Refuse any removal of a pre-existing installation.
            for cleanup_stage in ('remove', 'absence'):  # Run readback even when uninstall reports failure.
                try:  # Keep both cleanup outcomes independently observable.
                    actions[cleanup_stage]()  # Perform only ownership-limited cleanup or readback.
                    stages[cleanup_stage] = 'passed'  # The required cleanup gate completed successfully.
                except smoke_errors as error:  # Failure remains disqualifying even if another gate passes.
                    stages[cleanup_stage] = 'failed'  # Do not turn a query error into absence.
                    errors.append(cleanup_stage + ': ' + str(error))  # Retain the cleanup diagnostic.
    return stages, errors, owned  # The reporting layer also verifies private-state cleanup.


def mount_records():  # Read the kernel's actual mount table without invoking an external unmount tool.
    records = []  # Preserve each mount point and filesystem type.
    for line in Path('/proc/self/mountinfo').read_text(encoding='utf-8').splitlines():  # Read the current Linux mount namespace.
        fields = line.split()  # Mountinfo escapes spaces inside path fields.
        separator = fields.index('-')  # Locate the filesystem-specific part.
        path = re.sub(r'\\([0-7]{3})', lambda match: chr(int(match.group(1), 8)), fields[4])  # Decode kernel path escapes.
        records.append((Path(path), fields[separator + 1]))  # Keep exact mountpoint identity.
    return records  # Failure to read the table is never proof of unmounting.


def safe_remove_scope(path):  # Delete only an owned private scope, never mounted package contents.
    path = Path(path)  # Accept only the scope supplied by its creator.
    require(not any(mount == path or path in mount.parents for mount, _kind in mount_records()), 'owned scope still contains a mount: ' + str(path))  # Refuse recursive deletion through a live mount.
    require(not path.is_symlink(), 'owned scope was replaced by a symlink: ' + str(path))  # Do not follow a substituted root.
    if path.exists():  # An already removed owned directory needs no second deletion.
        shutil.rmtree(path)  # Remove only this run's created directory.
    require(not os.path.lexists(path), 'private smoke state survived cleanup: ' + str(path))  # Cleanup outcome is authoritative.


def host_case(arguments, receipt, log, package_format, configure):  # Share failure-safe host transaction and reporting.
    log = Path(log)  # Preserve existing log destinations.
    log.mkdir(parents=True, exist_ok=True)  # Retain diagnostics even for failed prerequisites.
    label = package_format + '-host'  # Preserve historical host log filenames.
    (log / (label + '.log')).write_text('', encoding='utf-8')  # Start a fresh transaction transcript.
    scopes = []  # Only newly created scopes may be recursively removed.
    stages, errors, owned = {}, [], False  # Initialize fail-closed outcome state.
    try:  # Report configuration and prerequisite failures like other native failures.
        require(os.geteuid() != 0, 'host smoke needs a disposable unprivileged account; use sudo only for package operations')  # Enforce the documented unprivileged lifecycle contract.
        path = requested_package(arguments, receipt, package_format)  # Validate the requested artifact before installation.
        work = Path(tempfile.mkdtemp(prefix='ilium-smoke-', dir=Path.home())).resolve()  # Keep Flatpak-visible state under the disposable user's home.
        scopes.append(work)  # Track ownership before subsequent setup can fail.
        environment = dict(os.environ)  # Preserve session-bus and system context needed by native managers.
        for key in list(environment):  # Remove ambient loader, application and sandbox overrides.
            if key.startswith(('LD_', 'APPIMAGE_', 'ILIUM_', 'FLATPAK_')) or key in ('APPDIR', 'APPIMAGE'):  # Prevent altered execution or sandbox helpers from qualifying.
                environment.pop(key)  # The smoke supplies its own application state explicitly.
        for key, leaf in (('HOME', 'home'), ('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'), ('TMPDIR', 'tmp')):  # Isolate application-owned writable paths.
            directory = work / leaf  # Every path is inside this run's owned scope.
            directory.mkdir(mode=0o700)  # Do not expose private state to another user.
            environment[key] = str(directory)  # Lifecycle will further isolate its state beneath these paths.
        environment.update(LC_ALL='C', ILIUM_SMOKE_BASE=str(work / 'tmp'))  # Stabilize manager parsing and lifecycle placement.
        if not environment.get('XDG_RUNTIME_DIR'):  # Keep a stable launcher runtime when no login session provided one.
            (work / 'runtime').mkdir(mode=0o700)  # Create only an owned private runtime directory.
            environment['XDG_RUNTIME_DIR'] = str(work / 'runtime')  # Flatpak wrappers must not inherit lifecycle's replacement runtime.
        write_smoke_files(work, receipt)  # Copy the unchanged lifecycle and exact cache verifier inputs.
        actions = configure(path, work, environment, scopes, label)  # Construct only known format-specific operations.
        stages, errors, owned = transaction(actions)  # Require install, byte/behavior and removal gates.
    except smoke_errors as error:  # Retain failures before the transaction could run.
        errors.append('setup: ' + str(error))  # Configuration failure cannot produce a passed result.
        stages['setup'] = 'failed'  # Distinguish it from a native package failure.
    finally:  # Private scope cleanup is also required before reporting success.
        can_clean = not owned or all(stages.get(stage) == 'passed' for stage in ('remove', 'absence'))  # Retain failed installations for diagnostics instead of erasing evidence.
        if can_clean:  # Remove private data only after authoritative package/mount teardown.
            for scope in reversed(scopes):  # Clean separately created Flatpak storage before its enclosing workspace.
                try:  # Treat private cleanup failure as a failed gate.
                    safe_remove_scope(scope)  # Never clean foreign data or a live mounted tree.
                except smoke_errors as error:  # Preserve cleanup diagnostics.
                    errors.append('state-cleanup: ' + str(error))  # A leftover owned scope cannot qualify success.
        stages['state_cleanup'] = 'passed' if can_clean and all(not os.path.lexists(scope) for scope in scopes) else 'failed'  # Require final filesystem readback.
    if stages.get('state_cleanup') != 'passed' and not errors:  # Guard an incomplete cleanup result even without an exception.
        errors.append('owned state cleanup is unverified')  # Explicitly fail incomplete cleanup.
    if not errors:
        try:
            name = packages.package_name(arguments.arch, package_format)
            require(packages.sha(requested_package(arguments, receipt, package_format)) ==
                    receipt['packages'][name],
                    'host package bytes changed during native animation smoke')
        except smoke_errors as error:
            errors.append('package-readback: ' + str(error))
    animation_digest = None
    if not errors:
        try:
            proof = release_tool.read_json(log / (label + '-installed-animation.json'))
            animation_digest = release_tool.digest(json.dumps(
                proof, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode('utf-8'))
        except smoke_errors as error:
            errors.append('animation-readback: ' + str(error))
    result = subprocess.CompletedProcess([], 1 if errors else 0, '', '\n'.join(errors))  # Preserve the existing report adapter.
    source_root = Path(arguments.workspace).resolve(strict=True).parent
    extra = {'arch': arguments.arch, 'tag': receipt['tag'], 'package': packages.package_name(arguments.arch, package_format), 'package_sha256': receipt['packages'].get(packages.package_name(arguments.arch, package_format)), 'source_archive_sha256': receipt['source_archive_sha256'], 'native_audit_sha256': packages.sha(arguments.audit_report), 'source_files': {name: packages.sha(source_root / name) for name in animation_gate.SOURCE_FILES}, 'animation_sha256': animation_digest, 'execution': {'appimage': 'fuse', 'flatpak': 'sandbox', 'snap': 'classic', 'deb': 'native-host'}[package_format], 'gates': stages, 'removed': stages.get('remove') == stages.get('absence') == 'passed', 'log': str(log / (label + '.log'))}  # Bind terminal result to source, audit, package and completed cleanup.
    return report('host', package_format, platform.platform(terse=True), result, extra)  # Only a complete transaction can report success.


def record_payload(log, label, package_format, tree, receipt):  # Retain installed-file hashes alongside command diagnostics.
    actual = verify_payload(package_format, tree, receipt)  # Verify the actual deployment or mounted image.
    with (Path(log) / (label + '.log')).open('a', encoding='utf-8') as output:  # Append to the complete transaction transcript.
        output.write(json.dumps({'installed_root': str(tree), 'installed_files': actual}, sort_keys=True) + '\n')  # Preserve exact readback evidence.


def expect_version(command, expected, log, label, environment):  # Check a real installed command's exact version result.
    result = checked([*command, '--version'], log, label, env=environment)  # A nonzero version command must fail too.
    require(result.stdout.rstrip('\n') == expected, 'installed version differs: expected %r, got %r' % (expected, result.stdout))  # Do not accept a different successful binary.


def installed_animation(arguments, receipt, log, label, package_format, payload_root,
                        executable_root, launcher, environment):
    supplied_audit = getattr(arguments, 'audit_report', None)
    audit_path = Path(supplied_audit).resolve() if supplied_audit is not None else None
    require(audit_path is not None and audit_path.is_file(),
            'native audit required for installed animation acceptance')
    output = Path(log) / (label + '-installed-animation.json')
    qualified = animation_gate.smoke(
        argparse.Namespace(workspace=Path(arguments.workspace).resolve(), root=Path(payload_root),
                           manifest=Path(arguments.manifest).resolve(), audit=audit_path,
                           output=output, os='linux', arch=arguments.arch, tag=receipt['tag'],
                           format=package_format, executable_root=Path(executable_root)),
        command=[str(part) for part in launcher] + ['release-animation-probe'],
        environment=environment,
        executor=lambda command, child_env, timeout: checked(command, log, label,
                                                               env=child_env, timeout=timeout))
    require(qualified['installed_files']['ilium'] == receipt['package_files']['ilium'] and
            qualified['installed_files']['ilium-animation-helper'] ==
            receipt['package_files']['ilium-animation-helper'],
            'installed animation receipt differs from package construction')
    with (Path(log) / (label + '.log')).open('a', encoding='utf-8') as transcript:
        transcript.write(json.dumps({'installed_animation_receipt': str(output),
                                     'sha256': packages.sha(output),
                                     'launcher_command': qualified['launcher_command'],
                                     'installed_root': qualified['installed_root']},
                                    sort_keys=True) + '\n')
    return qualified


def host_deb(arguments, receipt, log):  # Preserve the public host deb adapter signature.
    def configure(path, work, environment, scopes, label):  # Prepare a transaction on a disposable unprivileged account.
        reference = package_reference('deb', path, receipt, arguments.arch, work / 'reference')  # Reject arbitrary data or maintainer scripts before apt runs.
        verifier = work / 'package-deb.sh'  # Keep the complete installed-leaf verifier in the owned scope.
        verifier.write_text(reference_script(reference), encoding='utf-8')  # Include licenses and integrations bound to the package artifact.
        def absent():  # Prove manager and file absence with one failure-sensitive script.
            checked(['sh', work / 'absent-deb.sh'], log, label, env=environment)  # A database error is not an absent package.
        def install():  # Use privilege only for the package-manager mutation.
            checked([*sudo(), 'apt-get', 'install', '-y', '-qq', path], log, label, env={**environment, 'DEBIAN_FRONTEND': 'noninteractive'})  # Keep the exact supplied package install behavior.
        def verify():  # Verify installed bytes before the application lifecycle.
            identity = checked(['dpkg-query', '-W', '-f=${Status}\\n${Version}\\n${Architecture}\\n', 'ilium'], log, label, env=environment).stdout.splitlines()  # Read actual installed manager identity.
            require(identity == ['install ok installed', packages.deb_version(receipt['version']), packages.ARCHITECTURES[arguments.arch]['deb']], 'installed deb registration differs')  # Bind version and architecture as well as file bytes.
            checked(['sh', verifier, '/'], log, label, env=environment)  # Read back every admitted package-owned leaf.
            record_payload(log, label, 'deb', Path('/'), receipt)  # Include every audited runtime member and notices.
            verify_launchers(Path('/'), 'usr/bin')  # Bind both installed launcher paths to the pair.
            expect_version(['/usr/bin/ilium'], 'ilium ' + receipt['version'], log, label, environment)  # Check the actual installed client.
            expect_version(['/usr/bin/ilium-server'], 'ilium-server ' + receipt['version'], log, label, environment)  # Repair the missing server readback.
            expect_version(['/usr/lib/ilium/ilium-animation-helper'], release_tool.helper_version_record(receipt['version']), log, label, environment)
            installed_animation(arguments, receipt, log, label, 'deb', Path('/usr/lib/ilium'),
                                Path('/usr/lib/ilium'), ['/usr/bin/ilium'], environment)
            checked(['sh', work / 'lifecycle.sh', '/usr/bin/ilium'], log, label, env=environment)  # Preserve the real isolated lifecycle and retry.
        def remove():  # Removal success is an acceptance condition.
            checked([*sudo(), 'apt-get', 'remove', '-y', '-qq', 'ilium'], log, label, env={**environment, 'DEBIAN_FRONTEND': 'noninteractive'})  # Do not purge data or hide errors.
        return {'preflight': absent, 'install': install, 'verify': verify, 'remove': remove, 'absence': absent}  # Require the same fresh-state contract after uninstall.
    return host_case(arguments, receipt, log, 'deb', configure)  # Preserve count-based host result semantics.


def snap_inventory(log, label, environment):  # Read the complete Snap inventory through a successful daemon query.
    result = checked(['snap', 'list', '--all'], log, label, env=environment)  # Never turn snapd failure into absence.
    rows = result.stdout.splitlines()  # Machine parsing uses LC_ALL=C and nonterminal output.
    if not rows:  # A successful empty inventory contains no registration.
        return []  # The successful query is retained in the transcript.
    require(rows[0].split()[:3] == ['Name', 'Version', 'Rev'], 'unrecognized snap inventory output')  # Refuse to interpret malformed listings as empty.
    parsed = [line.split() for line in rows[1:] if line.strip()]  # Parse only complete table rows.
    require(all(len(row) >= 3 for row in parsed), 'incomplete snap inventory row')  # Preserve the revision needed for mounted-path binding.
    return parsed  # Callers match the exact package name.


def host_snap(arguments, receipt, log):  # Preserve classic Snap behavior with explicit owned-state preflight.
    def configure(path, work, environment, scopes, label):  # Capture only this transaction's state.
        require(platform.system() == 'Linux', 'Snap account custody requires Linux')  # Keep POSIX account access inside the native boundary.
        try:  # Importability on Windows must not weaken Linux account-data preflight.
            import pwd  # Acquire the real account database only for a Linux Snap transaction.
        except ImportError as error:  # An unavailable account inventory cannot qualify native Snap cleanup.
            raise release_tool.ReleaseError('Snap account custody requires the POSIX pwd module') from error  # Fail instead of assuming no user data exists.
        reference = package_reference('snap', path, receipt, arguments.arch, work / 'reference')  # Reject undeclared Snap hooks or data before snapd installs it.
        roots = [Path('/snap/ilium'), Path('/var/lib/snapd/snap/ilium')]  # Support the standard mount path and its documented distro symlink target.
        launchers = [Path('/snap/bin/ilium'), Path('/snap/bin/ilium.server'), Path('/snap/bin/ilium.helper'), Path('/var/lib/snapd/snap/bin/ilium'), Path('/var/lib/snapd/snap/bin/ilium.server'), Path('/var/lib/snapd/snap/bin/ilium.helper')]  # Include every declared command.
        data = {Path('/var/snap/ilium')} | {Path(entry.pw_dir) / 'snap/ilium' for entry in pwd.getpwall() if Path(entry.pw_dir).is_absolute()}  # Snap removal can touch data in real account homes despite overridden HOME.
        state = {}  # Retain the installed revision identity.
        def absent():  # Require a successful manager query and privileged path readback.
            require(not any(row[0] == 'ilium' for row in snap_inventory(log, label, environment)), 'Snap ilium is still registered')  # Refuse existing or remaining revisions.
            checked([*sudo(), 'sh', '-ec', absence_script([*roots, *launchers, *sorted(data)])], log, label, env=environment)  # Reject prior user data and detect all owned leftovers without reading their contents.
            checked([*sudo(), 'sh', '-ec', 'for path in /var/lib/snapd/snaps/ilium_*.snap; do test ! -e "$path"; test ! -L "$path"; done'], log, label, env=environment)  # A retained installed snap image also blocks absence.
        def preflight():  # Prove snapd readiness before assuming installation ownership.
            checked([*sudo(), 'snap', 'wait', 'system', 'seed.loaded'], log, label, env=environment)  # Preserve the documented seed readiness gate.
            absent()  # Existing registrations or data must never be adopted.
        def install():  # Keep the supplied local classic package install.
            checked([*sudo(), 'snap', 'install', '--dangerous', '--classic', path], log, label, env=environment)  # No store publication or strict-confinement claim is introduced.
        def verify():  # Bind reads and behavior to the actual installed revision.
            rows = [row for row in snap_inventory(log, label, environment) if row[0] == 'ilium']  # A fresh installation must have one exact revision.
            require(len(rows) == 1 and rows[0][1] == receipt['version'] and re.fullmatch(r'(?:x)?[0-9]+', rows[0][2]), 'installed Snap identity differs')  # Validate version and safe revision spelling.
            root = (Path('/snap/ilium') / rows[0][2]).resolve(strict=True)  # Resolve the actual mounted revision once.
            require((Path('/snap/ilium/current')).resolve(strict=True) == root, 'Snap current revision differs')  # Bind the manager launchers to the verified revision.
            state['root'] = root  # Retain its location for post-remove readback.
            stored = Path('/var/lib/snapd/snaps') / ('ilium_' + rows[0][2] + '.snap')  # snapd's revision store must contain this exact input image.
            stored_check = 'test -f %s\ntest ! -L %s\nsha256sum -- %s' % ((shlex.quote(str(stored)),) * 3)  # Read snapd's root-owned image without changing its permissions.
            stored_readback = checked([*sudo(), 'sh', '-ec', stored_check], log, label, env=environment).stdout  # Keep the regular-file and hash checks inside one privileged read-only command.
            require(stored_readback == '%s  %s\n' % (receipt['packages'][path.name], stored), 'stored Snap image differs from the input artifact')  # Bind the exact installed image and its path to the input digest.
            record_payload(log, label, 'snap', root, receipt)  # Read every installed audited member.
            require(layout_reference('snap', root, receipt) == reference, 'installed Snap package layout or ancillary bytes differ')  # Verify the complete admitted mounted data tree.
            require((root / 'meta/snap.yaml').read_text(encoding='utf-8') == packages.render_snap_yaml(receipt['version'], arguments.arch), 'installed Snap command metadata differs')  # Bind both command definitions to the supplied builder.
            expect_version(['snap', 'run', 'ilium'], 'ilium ' + receipt['version'], log, label, environment)  # Use snapd's actual client launcher.
            expect_version(['snap', 'run', 'ilium.server'], 'ilium-server ' + receipt['version'], log, label, environment)  # Use the actual sibling server launcher.
            expect_version(['snap', 'run', 'ilium.helper'], release_tool.helper_version_record(receipt['version']), log, label, environment)
            installed_animation(arguments, receipt, log, label, 'snap', root / 'lib/ilium',
                                root / 'lib/ilium', ['snap', 'run', 'ilium'], environment)
            checked(['sh', work / 'lifecycle.sh', 'snap', 'run', 'ilium'], log, label, env=environment)  # Preserve the real unprivileged classic lifecycle.
        def remove():  # Let snapd perform its documented synchronous uninstall.
            checked([*sudo(), 'snap', 'remove', '--purge', 'ilium'], log, label, env=environment)  # Avoid a new disposable-test snapshot; snapd retains pre-existing snapshots.
        def removed():  # Require registration, mounted payload, launcher and data absence.
            absent()  # A failed daemon query remains a failure.
            require('root' not in state or not os.path.lexists(state['root']), 'installed Snap revision survived removal')  # Check the recorded actual revision path too.
        return {'preflight': preflight, 'install': install, 'verify': verify, 'remove': remove, 'absence': removed}  # Both removal and readback determine success.
    return host_case(arguments, receipt, log, 'snap', configure)  # removed=false can no longer accompany a successful result.


def flatpak_wrapper(user_directory, host_environment, architecture, command=None):  # Pin installation selection across lifecycle HOME/XDG changes.
    fixed = ['env', 'FLATPAK_USER_DIR=' + str(user_directory)]  # Select only this run's private user installation.
    fixed += [key + '=' + host_environment[key] for key in ('HOME', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'TMPDIR') if key in host_environment]  # Keep launcher-side state stable and owned.
    if host_environment.get('XDG_RUNTIME_DIR'):  # Preserve the real session runtime needed by Flatpak itself.
        fixed.append('XDG_RUNTIME_DIR=' + host_environment['XDG_RUNTIME_DIR'])  # Do not replace the launcher's session context with lifecycle scratch.
    fixed += ['flatpak', 'run', '--user', '--arch=' + architecture, '--branch=' + packages.FLATPAK_BRANCH]  # Exclude same-ID system or other-architecture applications.
    if command is not None:  # The client exercises the metadata's default command.
        fixed.append('--command=' + command)  # Server-version readback uses the documented command override.
    return '#!/bin/sh\n' + 'exec ' + shlex.join(fixed) + ' --env=HOME="$HOME" --env=XDG_DATA_HOME="$XDG_DATA_HOME" --env=XDG_CONFIG_HOME="$XDG_CONFIG_HOME" --env=XDG_CACHE_HOME="$XDG_CACHE_HOME" --env=XDG_RUNTIME_DIR="$XDG_RUNTIME_DIR" --env=TMPDIR="$TMPDIR" ' + shlex.quote(packages.APP_ID) + ' "$@" # Run with fixed installation identity and isolated application state.\n'  # Shell expansion captures lifecycle state before env fixes the launcher.


def host_flatpak(arguments, receipt, log):  # Preserve the optional private-installation CLI argument.
    def configure(path, work, environment, scopes, label):  # Create only a fresh installation owned by this run.
        supplied = getattr(arguments, 'flatpak_user_dir', None)  # Keep callers that omit the optional field usable.
        user_directory = Path(supplied).absolute() if supplied else work / 'flatpak'  # A supplied path remains the actual installation directory.
        plain_directory(user_directory.parent)  # Refuse ownership through a symlinked caller-supplied parent.
        require(not os.path.lexists(user_directory), '--flatpak-user-dir must name a new directory')  # Never adopt or erase an existing user installation.
        user_directory.mkdir(mode=0o700)  # Its parent must already exist and remains caller-owned.
        if supplied:  # Track a separately located owned installation explicitly.
            scopes.append(user_directory)  # Cleanup never deletes its caller-owned parent.
        environment['FLATPAK_USER_DIR'] = str(user_directory)  # Keep all manager calls pinned independently of XDG state.
        architecture = packages.ARCHITECTURES[arguments.arch]['flatpak']  # Use the supplied Flatpak architecture mapping.
        reference = 'app/%s/%s/%s' % (packages.APP_ID, architecture, packages.FLATPAK_BRANCH)  # Bind the exact built application ref.
        wrappers = []  # Retain default client and explicit server sandbox commands.
        for leaf, command in (('flatpak-client.sh', None), ('flatpak-server.sh', 'ilium-server'), ('flatpak-helper.sh', 'ilium-animation-helper')):  # Avoid direct host execution of deployed binaries.
            wrapper = work / leaf  # Keep launchers in the owned visible workspace.
            wrapper.write_text(flatpak_wrapper(user_directory, environment, architecture, command), encoding='utf-8')  # Write complete pinned wrapper source.
            wrapper.chmod(0o755)  # Allow the unchanged lifecycle to execute the wrapper.
            wrappers.append(wrapper)  # Preserve deterministic client/server order.
        state = {}  # Retain the actual deployment location returned by Flatpak.
        def refs():  # A successful list is required before interpreting absence.
            result = checked(['flatpak', 'list', '--user', '--app', '--columns=application,arch,branch'], log, label, env=environment)  # Query unambiguous documented identity columns.
            rows = {tuple(line.split()) for line in result.stdout.splitlines() if line.strip()}  # Avoid assuming whether a display ref includes its kind prefix.
            require(all(len(row) == 3 for row in rows), 'unrecognized Flatpak inventory output')  # Never interpret malformed rows as absence.
            return rows  # Preserve all installed application identities.
        def absent():  # Require no application ref or installed app payload in the private installation.
            require(not refs(), 'private Flatpak application remains registered')  # Reject unexpected application refs as well as Ilium.
            app_root = user_directory / 'app' / packages.APP_ID  # Inspect only this application's deployment namespace.
            require(not os.path.lexists(app_root) or (app_root.is_dir() and not app_root.is_symlink() and all(path.is_dir() and not path.is_symlink() for path in app_root.rglob('*'))), 'Flatpak deployment files survived removal')  # Only real empty directory parents may remain.
            require('deployment' not in state or not os.path.lexists(state['deployment']), 'recorded Flatpak deployment survived removal')  # Check the actual installed location.
            removed_root = user_directory / '.removed'  # Flatpak can move a still-locked deployment here while reporting uninstall success.
            require(not os.path.lexists(removed_root) or (removed_root.is_dir() and not removed_root.is_symlink() and not list(removed_root.iterdir())), 'Flatpak retained a removed deployment')  # Never erase live/failed undeploy evidence through workspace cleanup.
            exported = ['bin/' + packages.APP_ID, 'share/applications/' + packages.APP_ID + '.desktop', 'share/metainfo/' + packages.APP_ID + '.metainfo.xml', 'share/icons/hicolor/256x256/apps/' + packages.APP_ID + '.png', 'share/icons/hicolor/scalable/apps/' + packages.APP_ID + '.svg']  # Include package-specific exported launchers and integrations.
            require(all(not os.path.lexists(user_directory / 'exports' / name) for name in exported), 'Flatpak exported installation entries survived removal')  # Dangling exports also count as leftovers.
        def install():  # Preserve dependency acquisition inside the private installation.
            checked(['flatpak', 'remote-add', '--user', '--if-not-exists', 'flathub', 'https://dl.flathub.org/repo/flathub.flatpakrepo'], log, label, env=environment)  # Never modify a real user's configured remotes.
            checked(['flatpak', 'install', '--user', '-y', '--noninteractive', '--bundle', path], log, label, env=environment)  # Install the exact local bundle and its required runtime.
        def verify():  # Prove deployed bytes and actual sandbox behavior.
            require(refs() == {(packages.APP_ID, architecture, packages.FLATPAK_BRANCH)}, 'installed Flatpak ref differs from the requested architecture/branch')  # Prevent default-ref or system-installation substitution.
            location = checked(['flatpak', 'info', '--user', '--show-location', reference], log, label, env=environment).stdout.strip()  # Read the documented deployed location.
            deployment = Path(location)  # Do not invent a commit-directory path.
            require(deployment.is_absolute() and user_directory.resolve() in deployment.resolve(strict=True).parents, 'Flatpak deployment escaped the private installation')  # Confine owned readbacks.
            state['deployment'] = deployment.resolve(strict=True)  # Retain the actual deployment for removal verification.
            record_payload(log, label, 'flatpak', state['deployment'] / 'files', receipt)  # Read the complete installed audited inventory.
            layout_reference('flatpak', state['deployment'] / 'files', receipt)  # Reject any arbitrary executable or data outside the audited runtime namespace.
            verify_launchers(state['deployment'] / 'files', 'bin')  # Bind both sandbox commands to audited binaries.
            metadata = configparser.ConfigParser(interpolation=None)  # Parse only documented Flatpak application metadata.
            metadata.read(state['deployment'] / 'metadata', encoding='utf-8')  # Read metadata from the verified private deployment.
            require(metadata.get('Application', 'name', fallback='') == packages.APP_ID and metadata.get('Application', 'command', fallback='') == 'ilium', 'installed Flatpak command metadata differs')  # The default client must be the supplied command.
            policy = release_tool.read_json(packages.PACKAGING / 'tools.json')['flatpak']  # Preserve the supplied runtime policy instead of inventing a replacement.
            require(metadata.get('Application', 'runtime', fallback='') == '%s/%s/%s' % (policy['runtime'], architecture, policy['runtime_version']), 'installed Flatpak runtime differs')  # Bind the selected sandbox runtime family and branch.
            expect_version([wrappers[0]], 'ilium ' + receipt['version'], log, label, environment)  # Execute the client inside the real sandbox.
            expect_version([wrappers[1]], 'ilium-server ' + receipt['version'], log, label, environment)  # Execute the installed server inside that sandbox.
            expect_version([wrappers[2]], release_tool.helper_version_record(receipt['version']), log, label, environment)
            installed_animation(arguments, receipt, log, label, 'flatpak',
                                state['deployment'] / 'files/lib/ilium', Path('/app/lib/ilium'),
                                [wrappers[0]], environment)
            checked(['sh', work / 'lifecycle.sh', wrappers[0]], log, label, env=environment)  # Preserve actual sandbox lifecycle without bypassing restrictions.
        def remove():  # Uninstall only the exact app ref created by this run.
            checked(['flatpak', 'uninstall', '--user', '-y', '--noninteractive', '--no-related', reference], log, label, env=environment)  # Never use --delete-data or remove unrelated installations.
        return {'preflight': absent, 'install': install, 'verify': verify, 'remove': remove, 'absence': absent}  # Missing sandbox support or failed cleanup must fail.
    return host_case(arguments, receipt, log, 'flatpak', configure)  # Private runtime storage is cleaned only after authoritative application uninstall.


def stop_mount(process):  # Reap only the retained AppImage mount child.
    if process.poll() is not None:  # A reaped child is never signaled through a saved PID.
        process.wait(timeout=10)  # Preserve explicit child-reap accounting.
        return  # The independent mount-table check still determines unmount success.
    process.send_signal(signal.SIGINT)  # Use the documented interruption contract for --appimage-mount.
    try:  # Bound graceful mount shutdown.
        process.wait(timeout=10)  # Reap the owned foreground child.
    except subprocess.TimeoutExpired:  # Escalate only this retained child's teardown.
        process.terminate()  # Never signal an unrelated saved PID or application name.
        try:  # Keep termination bounded too.
            process.wait(timeout=10)  # Reap before releasing process ownership.
        except subprocess.TimeoutExpired:  # A stuck owned child still cannot block forever.
            process.kill()  # Limit the final signal to the retained child handle.
            process.wait(timeout=10)  # Require bounded reap completion.


def host_appimage(arguments, receipt, log):  # Preserve the host adapter while requiring actual FUSE evidence.
    def configure(path, work, environment, scopes, label):  # Own a private image, cache and mount process.
        reference = package_reference('appimage', path, receipt, arguments.arch, work / 'reference')  # Reject arbitrary AppDir payloads before executing its pinned runtime.
        image = work / 'ilium.AppImage'  # Never chmod or remove the supplied artifact.
        mount_output = Path(log) / 'appimage-mount.stdout.log'  # Retain raw mount output independently of workspace cleanup.
        mount_error = Path(log) / 'appimage-mount.stderr.log'  # Retain failed-mount diagnostics too.
        state = {}  # Keep the live Popen handle and observed mountpoint together.
        def preflight():  # Existence alone is only a preliminary prerequisite check.
            require(Path('/dev/fuse').exists() and stat.S_ISCHR(Path('/dev/fuse').stat().st_mode), 'a real /dev/fuse device is required')  # Missing FUSE is a failed requested gate.
            require(shutil.which('fusermount3') or shutil.which('fusermount'), 'a FUSE mount helper is required')  # Do not silently substitute extract-and-run.
        def install():  # Start the documented mount operation using exact package bytes.
            shutil.copyfile(path, image)  # Keep the input package immutable.
            image.chmod(0o755)  # Execute only this run's private copy.
            require(packages.sha(image) == receipt['packages'][path.name], 'private AppImage bytes differ')  # Bind the executed mount runtime to the artifact.
            with mount_output.open('w', encoding='utf-8') as stdout, mount_error.open('w', encoding='utf-8') as stderr:  # Preserve output even on failed startup.
                state['process'] = subprocess.Popen([str(image), '--appimage-mount'], stdout=stdout, stderr=stderr, env=environment)  # Retain foreground child custody.
            deadline = time.monotonic() + 30  # Bound mount readiness without blocking on readline.
            while time.monotonic() < deadline:  # Wait only for this owned runtime's mount report.
                lines = mount_output.read_text(encoding='utf-8').splitlines()  # Poll retained output without holding a pipe open.
                require(state['process'].poll() is None, 'AppImage mount runtime exited before readiness')  # Premature child exit cannot prove FUSE.
                if lines:  # Validate the runtime's first complete mount-path line.
                    mount = Path(lines[0])  # Use the runtime-provided mount path rather than an invented directory.
                    require(mount.is_absolute(), 'AppImage mount path is not absolute')  # Reject malformed runtime output.
                    if any(point == mount and kind.startswith('fuse') for point, kind in mount_records()):  # Require an actual FUSE mount in the kernel table.
                        state['mount'] = mount  # Retain the verified mount identity until teardown.
                        with (Path(log) / (label + '.log')).open('a', encoding='utf-8') as output:  # Retain the observed native mount evidence.
                            output.write(json.dumps({'fuse_mount': str(mount), 'owned_mount_pid': state['process'].pid, 'mount_records': [(str(point), kind) for point, kind in mount_records() if point == mount]}) + '\n')  # Bind the kernel readback to the retained child.
                        return  # Byte verification runs while this mount remains held.
                time.sleep(0.05)  # Avoid spinning while the owned mount initializes.
            raise release_tool.ReleaseError('AppImage FUSE mount readiness timed out')  # Fail instead of extracting as a fallback.
        def verify():  # Verify the held mounted payload and every executed cache.
            record_payload(log, label, 'appimage', state['mount'], receipt)  # Include the notice left in the AppDir.
            require(layout_reference('appimage', state['mount'], receipt) == reference, 'mounted AppImage layout or ancillary bytes differ')  # Read back the entire admitted AppDir.
            verify_launchers(state['mount'], 'usr/bin')  # Check both AppDir launcher links.
            wrapper = work / 'appimage-client.sh'  # Preserve the lifecycle command-array interface.
            wrapper.write_text(appimage_wrapper(image, work, receipt['version']), encoding='utf-8')  # Verify each actual materialized cache after invocation.
            wrapper.chmod(0o755)  # Permit the unchanged lifecycle to run it.
            expect_version([wrapper], 'ilium ' + receipt['version'], log, label, environment)  # Wrapper also checks cached bytes and the exact server version.
            cached = Path(environment['XDG_DATA_HOME']) / 'ilium/appimage' / (work / 'identity').read_text(encoding='ascii').strip()
            installed_animation(arguments, receipt, log, label, 'appimage', cached, cached,
                                [wrapper], environment)
            checked(['sh', work / 'lifecycle.sh', wrapper], log, label, env=environment)  # Exercise the real FUSE-backed launcher through the complete lifecycle.
            require(state['process'].poll() is None, 'held AppImage mount exited during acceptance')  # Keep mounted readback evidence tied to a live owned process.
        def remove():  # Teardown runs after failed installation or behavior as well as success.
            if 'process' in state:  # A failed Popen startup produced no owned child.
                stop_mount(state['process'])  # Interrupt and reap only this mount's retained child.
            if image.exists():  # Remove only the private executable copy.
                image.unlink()  # The original package remains in the artifact inventory.
        def absent():  # Kernel readback is authoritative even if the child already exited.
            require('process' not in state or state['process'].poll() is not None, 'AppImage mount child was not reaped')  # Verify process teardown.
            mounts = mount_records()  # A failed mount-table query fails removal evidence.
            deadline = time.monotonic() + 10  # The FUSE daemon's kernel teardown may lag the retained launcher exit.
            while any(point == state.get('mount') or point == work or work in point.parents for point, _kind in mounts) and time.monotonic() < deadline:
                time.sleep(0.05)  # Wait only for mounts belonging to this private scope; never detach a foreign mount.
                mounts = mount_records()  # Actual kernel absence, rather than elapsed time, is still required.
            require('mount' not in state or not any(point == state['mount'] for point, _kind in mounts), 'AppImage FUSE mount survived teardown')  # A dead launcher alone does not prove unmounting.
            require(not any(point == work or work in point.parents for point, _kind in mounts), 'owned AppImage workspace still has a mount')  # Catch a late mount whose stdout readiness was missed.
            require(not os.path.lexists(image), 'private AppImage survived removal')  # Cached state is removed and checked by host_case before reporting.
        return {'preflight': preflight, 'install': install, 'verify': verify, 'remove': remove, 'absence': absent}  # FUSE and cleanup failures cannot qualify.
    return host_case(arguments, receipt, log, 'appimage', configure)  # Preserve the existing count-based public adapter.


def host(arguments):  # Validate all runtime coverage before any host installation.
    formats = selected_formats(arguments)  # Reject unsupported, empty or duplicate requests early.
    native_architecture(arguments.arch)  # Preserve and strengthen the existing architecture gate.
    receipt = load_receipt(arguments.packages, arguments.arch)  # Rehash all supplied package artifacts.
    target = release_tool.selected_target(Path(arguments.manifest),
                                          arguments.arch + '-unknown-linux-gnu')
    audit = release_tool.audit_receipt(Path(arguments.audit_report), target,
                                       receipt['version'], receipt['tag'])
    require(audit['files'] == receipt['package_files'],
            'host package inventory differs from native audit')
    for package_format in formats:  # Validate every requested package before installing the first.
        requested_package(arguments, receipt, package_format)  # Missing artifacts cannot produce partial qualified coverage.
    runners = {'snap': host_snap, 'appimage': host_appimage, 'flatpak': host_flatpak, 'deb': host_deb}  # Keep the supplied host API inventory.
    return sum(runners[package_format](arguments, receipt, arguments.log.resolve()) for package_format in formats)  # Every requested gate contributes to the final status.


def parser():
    result = release_tool.JsonArgumentParser(description=__doc__, allow_abbrev=False)
    commands = result.add_subparsers(dest='command', required=True)
    for name in ('inspect', 'containers', 'host'):
        command = commands.add_parser(name, allow_abbrev=False)
        command.add_argument('--arch', required=True, choices=sorted(packages.ARCHITECTURES))
        command.add_argument('--packages', type=Path, required=True)
        command.add_argument('--formats', default=','.join(mode_formats[name]))  # Defaults declare only the mode's actual supported coverage.
        if name != 'inspect':
            command.add_argument('--log', type=Path, required=True)
        if name in ('host', 'containers'):
            command.add_argument('--audit-report', type=Path, required=True)
            command.add_argument('--workspace', type=Path, required=True)
            command.add_argument('--manifest', type=Path, required=True)
        if name == 'host':
            command.add_argument('--flatpak-user-dir', type=Path, help='new private installation directory; its parent must exist')  # Never adopt an existing installation.
    return result


def main(argv=None):
    try:
        arguments = parser().parse_args(argv)
        failed = {'inspect': inspect, 'containers': containers, 'host': host}[arguments.command](arguments)
        native_identity = ({'system': platform.system(), 'machine': platform.machine()}
                           if arguments.command in ('containers', 'host') else None)
        emit('summary', command=arguments.command, arch=arguments.arch,
             tag=(load_receipt(arguments.packages, arguments.arch)['tag']
                  if arguments.command in ('containers', 'host') else None),
             native_identity=native_identity,
             state='failed' if failed else 'passed', failed=failed)
        return 1 if failed else 0
    except smoke_errors as error:  # Preserve structured failure for every supported inspection/manager error.
        emit('error', message=str(error)[:1200])
        return 1


if __name__ == '__main__':
    sys.exit(main())
