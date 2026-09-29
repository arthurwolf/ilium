#!/usr/bin/env python3
"""Evidence-bound native release orchestration. Python 3.11 standard library.

Remote mutations exist only in explicit draft/publish/latest/deploy subcommands.
The workflow gates those commands to tag pushes. No confirmation or manual
review step can replace native receipts. Source and dispatch paths never publish.
"""
from __future__ import annotations

import argparse
import hashlib
import copy
import json
import math
import os
from pathlib import Path, PureWindowsPath
import platform
import re
import shutil
import subprocess
import sys
import uuid
import tarfile
import urllib.error
import urllib.parse
import urllib.request

import pages
import release_tool

ROOT = Path(__file__).resolve().parents[2]
REPOSITORY = 'arthurwolf/ilium'
ORIGIN = 'https://github.com/' + REPOSITORY + '/releases'
POSIX_COMMAND = "curl --proto '=https' --tlsv1.2 -LsSf https://ilium-setup.pages.dev/install.sh | sh"
WINDOWS_COMMAND = 'irm https://ilium-setup.pages.dev/install.ps1 | iex'
WRANGLER = 'wrangler@4.38.0'
MAX_DOWNLOAD = 1_073_741_824
PTY_TESTS = ('attaching_tui_renders_the_pane_created_by_new_pane_and_responds_to_the_help_keystroke', 'right_click_restart_reloads_only_the_client_and_preserves_the_server')


def require(condition, message):
    if not condition:
        raise release_tool.ReleaseError(message)


def emit(kind, **values):
    release_tool.emit({'type': kind, **values})


def sha(path):
    path = Path(path)
    require(path.is_file() and not path.is_symlink(), 'digest input must be a regular file')
    value = hashlib.sha256()
    with Path(path).open('rb') as source:
        while block := source.read(1024 * 1024):
            value.update(block)
    return value.hexdigest()


def write_json(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('x', encoding='utf-8') as destination:
        json.dump(value, destination, sort_keys=True, indent=2)
        destination.write('\n')
        destination.flush(); os.fsync(destination.fileno())


def evidence_file_hashes(native_directory):
    native_directory = Path(native_directory)
    evidence = native_directory / 'evidence'
    require(evidence.is_dir() and not evidence.is_symlink(), 'native evidence directory must be regular')
    values = {}
    for path in sorted(evidence.rglob('*')):
        require(not path.is_symlink() and (path.is_dir() or path.is_file()),
                'native evidence contains a link or special entry')
        if path.is_file():
            relative = path.relative_to(native_directory).as_posix()
            require(relative not in values, 'duplicate native evidence path')
            values[relative] = sha(path)
    require(values, 'native evidence inventory cannot be empty')
    return values


def evidence_files_digest(values):
    require(isinstance(values, dict) and values, 'native evidence hash map is missing')
    return release_tool.digest(json.dumps(values, sort_keys=True, separators=(',', ':')).encode('utf-8'))


def validate_retained_windows_cmake(receipt_path, cache_path):
    receipt = release_tool.read_json(receipt_path)
    cache = receipt.get('cmake_cache', {})
    require(cache.get('sha256') == sha(cache_path), 'retained Windows CMake cache hash differs from source receipt')
    from build_windows_ort import parse_cmake_cache
    actual = parse_cmake_cache(cache_path)
    recorded = cache.get('values')
    require(isinstance(recorded, dict) and recorded and
            all(actual.get(name) == value for name, value in recorded.items()),
            'retained Windows CMake cache values differ from source receipt')
    return {'path': 'evidence/windows-ort-CMakeCache.txt', 'sha256': sha(cache_path)}


def gh_output(**values):
    destination = os.environ.get('GITHUB_OUTPUT')
    if destination:
        with open(destination, 'a', encoding='utf-8') as output:
            for name, value in values.items():
                text = str(value)
                require('\n' not in text and '\r' not in text, 'workflow output must be one line')
                output.write(name + '=' + text + '\n')


def workspace_version(workspace):
    import tomllib
    version = tomllib.loads(Path(workspace).read_text(encoding='utf-8'))['workspace']['package']['version']
    require(isinstance(version, str) and re.fullmatch(pages.TAG_PATTERN, 'v' + version), 'invalid workspace version')
    return version


def git_identity(root):
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
    require(re.fullmatch('[0-9a-f]{40}', commit), 'source commit must be full SHA-1')
    require(not subprocess.check_output(['git', 'status', '--porcelain', '--untracked-files=no'], cwd=root, text=True).strip(), 'tracked source is dirty')
    expected = os.environ.get('GITHUB_SHA')
    require(not expected or expected == commit, 'checkout differs from workflow source commit')
    return commit


def source_matrix(manifest, workspace, tag):
    # Consume the actual manifest CLI, including its strict five-target policy.
    response = subprocess.run([sys.executable, str(ROOT / 'release/scripts/release_tool.py'), 'targets', '--manifest', str(Path(manifest).resolve())], capture_output=True, text=True, check=True)
    records = [json.loads(line) for line in response.stdout.splitlines()]
    require(len(records) == 1 and records[0].get('type') == 'result', 'targets CLI returned unexpected records')
    targets = records[0]['targets']
    require(targets == release_tool.load_targets(manifest), 'targets CLI differs from authoritative manifest')
    version_tag = 'v' + workspace_version(workspace)
    require(not tag or tag == version_tag, 'tag differs from workspace version')
    git_identity(Path(workspace).resolve().parent)
    return {'include': targets}, version_tag


def source(arguments):
    matrix, tag = source_matrix(arguments.manifest, arguments.workspace, arguments.tag)
    commit = git_identity(arguments.workspace.resolve().parent)
    gh_output(matrix=json.dumps(matrix, separators=(',', ':')), tag=tag, commit=commit)
    emit('result', command='source', matrix=matrix, tag=tag, commit=commit)


class HTTPSOnly(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, new_url):
        require(urllib.parse.urlparse(new_url).scheme == 'https', 'insecure redirect rejected')
        if request.has_header('Authorization'):
            require(urllib.parse.urlparse(request.full_url).netloc == urllib.parse.urlparse(new_url).netloc, 'cross-origin authorized redirect rejected')
        return super().redirect_request(request, response, code, message, headers, new_url)


def request(url, *, method='GET', data=None, headers=None, limit=MAX_DOWNLOAD):
    require(urllib.parse.urlparse(url).scheme == 'https', 'HTTPS is required')
    request_headers = {'User-Agent': 'ilium-release-pipeline', **(headers or {})}
    try:
        with urllib.request.build_opener(HTTPSOnly()).open(urllib.request.Request(url, data=data, headers=request_headers, method=method), timeout=90) as response:
            content = response.read(limit + 1)
            require(len(content) <= limit, 'bounded download exceeded')
            return content, response.headers
    except urllib.error.HTTPError as error:
        # Never report authorization or the provider's potentially sensitive body.
        raise HTTPFailure(error.code) from None


def download(url, destination, expected_sha):
    require(re.fullmatch('[0-9a-f]{64}', expected_sha), 'download needs reviewed SHA-256')
    content, _headers = request(url)
    require(release_tool.digest(content) == expected_sha, 'download differs from reviewed SHA-256')
    destination = Path(destination)
    destination.parent.mkdir(parents=True, exist_ok=True)
    with destination.open('xb') as output:
        output.write(content)
    emit('artifact', path=str(destination.resolve()), sha256=expected_sha)


def logged(command, root, log, environment=None, timeout=10_800):
    emit('progress', operation=Path(str(command[0])).name, log=str(log))
    log = Path(log)
    with log.open('xb') as output:
        result = subprocess.run(list(map(str, command)), cwd=root, env=environment, stdout=output, stderr=subprocess.STDOUT, timeout=timeout, check=False)
    require(result.returncode == 0, 'command failed; inspect owned log ' + str(log))
    emit('result', operation=Path(str(command[0])).name, state='passed', log=str(log), log_sha256=sha(log))


def native(arguments):
    target = release_tool.selected_target(arguments.manifest, arguments.target)
    root = arguments.workspace.resolve().parent
    version = release_tool.workspace_version(arguments.workspace, arguments.tag)
    expected_system = {'linux': 'Linux', 'windows': 'Windows', 'macos': 'Darwin'}[target['os']]
    expected_machines = {'x86_64': {'x86_64', 'AMD64'}, 'aarch64': {'aarch64', 'arm64', 'ARM64'}}[target['arch']]
    require(platform.system() == expected_system and platform.machine() in expected_machines, 'native runner does not match target')
    require(arguments.runner_identity == target['runner'], 'runner identity differs from manifest')
    work = arguments.work.resolve()
    require(not work.exists(), 'native work directory must be new')
    work.mkdir(parents=True, mode=0o700)
    if hasattr(os, 'nice'):
        os.nice(10)
    environment = dict(os.environ)
    environment.update(CARGO_INCREMENTAL='0')
    # Ambient ORT overrides cannot select an unreviewed runtime. Intel source
    # builds and the shared-runtime adapter supply their own exact settings.
    for key in ('ORT_LIB_LOCATION', 'ORT_LIB_PATH', 'ORT_PREFER_DYNAMIC_LINK'):
        environment.pop(key, None)
    if target['os'] == 'windows':
        for key in ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS'):
            environment.pop(key, None)
    if target['os'] == 'windows':
        probe_target = work / 'symlink-privilege-target'
        probe_link = work / 'symlink-privilege-link'
        probe_target.write_bytes(b'owned Windows contract prerequisite')
        try:
            probe_link.symlink_to(probe_target)
        except OSError as error:
            raise release_tool.ReleaseError('Windows native contract tests require real symlink privilege or developer mode: ' + str(error)) from error
        require(probe_link.is_symlink(), 'Windows symlink prerequisite did not create a real link')
        probe_link.unlink(); probe_target.unlink()
    if target['os'] == 'linux':
        environment['OPENSSL_STATIC'] = '1'
    cargo_home = work / 'cargo-home'
    cargo_target = work / 'cargo-target'
    ort_register = release_tool.read_json(root / 'release/ort-source.json')
    require(ort_register.get('state') == 'reviewed', 'ORT source register is not reviewed')
    ort_archive = work / 'onnxruntime-source.tar.gz'
    download(ort_register['source_url'], ort_archive, ort_register['source_sha256'])
    model_register = release_tool.read_json(root / 'release/embedding-model.json')
    require(model_register.get('reviewed') is True, 'embedding model register is not reviewed')
    model_directory = work / 'model'; model_directory.mkdir()
    for name, digest in model_register['files'].items():
        require(re.fullmatch('[A-Za-z0-9._-]+', name), 'unsafe model filename')
        download('https://huggingface.co/' + model_register['repository'] + '/resolve/' + model_register['revision'] + '/' + name, model_directory / name, digest)
    if target['ort_strategy'] == 'pinned-source-build':
        helper = 'build_intel_ort.py' if target['os'] == 'macos' else 'build_windows_ort.py'
        output_name = 'intel-ort' if target['os'] == 'macos' else 'windows-ort'
        ort_output = work / output_name
        logged([sys.executable, root / 'release/scripts' / helper, '--source-register', root / 'release/ort-source.json', '--source-archive', ort_archive, '--output-root', ort_output, '--output', ort_output / 'receipt.json', '--cargo-environment-output', ort_output / 'environment.json', '--cargo-workspace', arguments.workspace.resolve(), '--cargo-target-dir', cargo_target, '--cargo-home', cargo_home, '--runner-identity', target['runner'], '--parallel', str(max(1, min(os.cpu_count() or 1, 4)))], root, work / (output_name + '.log'))
        environment.update(release_tool.read_json(ort_output / 'environment.json'))
        if target['os'] == 'windows':
            environment['PATH'] = environment['ORT_LIB_LOCATION'] + os.pathsep + environment['PATH']
    else:
        cargo_home.mkdir(); cargo_target.mkdir()
        environment.update(CARGO_HOME=str(cargo_home), CARGO_TARGET_DIR=str(cargo_target))
        if target['os'] in ('linux', 'macos'):
            shared_register = root / 'release/ort-runtime.json'
            policy = release_tool.read_json(shared_register)
            require(policy.get('schema') == 1 and policy.get('state') == 'reviewed', 'shared ORT runtime source policy is not reviewed')
            asset = policy['assets'][arguments.target]
            shared_archive = work / asset['name']
            download(asset['url'], shared_archive, asset['sha256'])
            require(shared_archive.stat().st_size == asset['bytes'], 'shared ORT archive size differs from pinned source policy')
            shared_output = work / 'shared-ort'
            logged([sys.executable, root / 'release/scripts/ort_runtime.py', '--register', shared_register, '--source-register', root / 'release/ort-source.json', '--target', arguments.target, '--archive', shared_archive, '--output-directory', shared_output], root, work / 'shared-ort.log')
            shared_receipt = release_tool.read_json(shared_output / 'ort-runtime-receipt.json')
            require(shared_receipt.get('state') == 'extracted-not-qualified' and shared_receipt.get('target') == arguments.target and shared_receipt.get('archive_sha256') == asset['sha256'] and shared_receipt.get('register_sha256') == sha(shared_register), 'shared ORT extraction is not bound to reviewed target/source')
            environment.update(shared_receipt['environment'])
            ort_library_directory = Path(shared_receipt['runtime_directory'])
            require(ort_library_directory.is_absolute() and ort_library_directory.is_dir(), 'shared ORT library directory is invalid')
            if target['os'] == 'macos':
                environment['DYLD_LIBRARY_PATH'] = str(ort_library_directory)
            elif target['os'] == 'linux':
                existing_loader_path = environment.get('LD_LIBRARY_PATH', '')
                environment['LD_LIBRARY_PATH'] = str(ort_library_directory) + (os.pathsep + existing_loader_path if existing_loader_path else '')
            else:
                environment['PATH'] = str(ort_library_directory) + os.pathsep + environment['PATH']
        logged(['cargo', 'build', '--locked', '--release', '--target', arguments.target, '--bin', 'ilium', '--bin', 'ilium-server'], root, work / 'release-build.log', environment)
    logged(['cargo', 'fmt', '--all', '--check'], root, work / 'fmt.log', environment)
    logged(['cargo', 'clippy', '--locked', '--workspace', '--all-targets', '--target', arguments.target, '--', '-D', 'warnings'], root, work / 'clippy.log', environment)
    logged(['cargo', 'test', '--locked', '--workspace', '--no-fail-fast', '--target', arguments.target], root, work / 'workspace-tests.log', environment)
    # Product tests above run in full on every native OS. POSIX fixture tests
    # require GNU/Linux tools; their Linux lanes run the complete fixture suite.
    # Portable archive/manifest/source/evidence contracts run on every lane.
    for test_module in sorted((root / 'release/tests').glob('test_*.py')):
        if test_module.name == 'test_posix_install.py' and target['os'] != 'linux':
            emit('result', suite=test_module.name, applicability='GNU/Linux fixture transport; covered by both Linux native lanes')
            continue
        logged([sys.executable, '-m', 'unittest', 'discover', '-s', 'release/tests', '-p', test_module.name], root, work / (test_module.stem + '.log'), environment)
    if target['os'] == 'windows':
        pester_setup = "[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; Install-PackageProvider -Name NuGet -MinimumVersion 2.8.5.201 -Scope CurrentUser -Force; Set-PSRepository -Name PSGallery -InstallationPolicy Trusted; Install-Module -Name Pester -RequiredVersion 5.7.1 -Scope CurrentUser -Force"
        logged(['powershell.exe', '-NoProfile', '-NonInteractive', '-Command', pester_setup], root, work / 'pester-setup.log', environment)
        for shell in ('powershell.exe', 'pwsh.exe'):
            pester_command = "Import-Module Pester -RequiredVersion 5.7.1 -Force; $result = Invoke-Pester -Path release/tests/Install.Tests.ps1 -Output Detailed -PassThru; if ($result.FailedCount -gt 0 -or $result.PassedCount -lt 1) { exit 1 }"
            logged([shell, '-NoProfile', '-NonInteractive', '-Command', pester_command], root, work / (shell + '-pester.log'), environment)
            logged([shell, '-NoProfile', '-File', root / 'release/tests/Test-WindowsInstallerContracts.ps1'], root, work / (shell + '-installer-contracts.log'), environment)
    logged(['cargo', 'fetch', '--locked'], root, work / 'cargo-fetch.log', environment)
    registries = list((cargo_home / 'registry/src').glob('*'))
    require(len(registries) == 1, 'expected exactly one cargo registry source')
    inventory = work / 'dependency-inventory.json'
    logged([sys.executable, root / 'release/scripts/licence_inventory.py', '--workspace', root, '--registry-source', registries[0], '--source-register', root / 'release/licence-sources.json', '--notice-directory', work / 'licence-notices', '--output', inventory], root, work / 'licences.log', environment)
    harness_log = work / 'harness-build.jsonl'
    logged(['cargo', 'test', '--locked', '--target', arguments.target, '--package', 'ilium', '--test', 'pty_tui_smoke', '--no-run', '--message-format=json'], root, harness_log, environment)
    harnesses = []
    for line in harness_log.read_text(encoding='utf-8').splitlines():
        if not line.startswith('{'):
            continue
        record = json.loads(line)
        if record.get('reason') == 'compiler-artifact' and record.get('target', {}).get('name') == 'pty_tui_smoke' and record.get('executable'):
            harnesses.append(Path(record['executable']))
    require(len(harnesses) == 1 and harnesses[0].is_file(), 'missing unique native PTY harness')
    command = [sys.executable, root / 'release/scripts/native_candidate.py', '--manifest', arguments.manifest.resolve(), '--workspace', arguments.workspace.resolve(), '--tag', arguments.tag, '--target', arguments.target, '--runner-identity', arguments.runner_identity, '--build-directory', cargo_target / arguments.target / 'release', '--output-directory', arguments.output.resolve(), '--dependency-inventory', inventory, '--ort-source-archive', ort_archive, '--model-directory', model_directory]
    if target['ort_strategy'] == 'pinned-source-build':
        if target['os'] == 'macos':
            command.extend(['--intel-ort-report', work / 'intel-ort/receipt.json'])
        else:
            command.extend(['--windows-ort-report', work / 'windows-ort/receipt.json'])
    if target['os'] in ('linux', 'macos') and target['ort_strategy'] != 'pinned-source-build':
        # Linux and macOS ARM use an explicit, SHA-pinned shared runtime
        # discovery root. Windows and Intel macOS use pinned source receipts.
        runtime_root = work / 'candidate-runtime'
        shutil.copytree(ort_library_directory, runtime_root / 'ort')
        command.extend(['--runtime-directory', runtime_root])
    if target['os'] == 'windows':
        vswhere = Path(os.environ['ProgramFiles(x86)']) / 'Microsoft Visual Studio/Installer/vswhere.exe'
        tools = subprocess.check_output([str(vswhere), '-latest', '-products', '*', '-find', 'VC/Tools/MSVC/**/bin/Hostx64/x64/dumpbin.exe'], text=True).splitlines()
        require(len(tools) == 1, 'cannot resolve unique native dumpbin')
        command.extend(['--dumpbin', tools[0]])
    logged(command, root, work / 'native-candidate.log', environment)
    output = arguments.output.resolve()
    if target['os'] in ('linux', 'macos') and target['ort_strategy'] != 'pinned-source-build':
        shutil.copyfile(shared_output / 'ort-runtime-receipt.json', output / 'evidence/ort-runtime-receipt.json')
    harness_name = 'native-test-binary.exe' if target['os'] == 'windows' else 'native-test-binary'
    harness_directory = output / 'evidence/harness'
    harness_directory.mkdir()
    shutil.copyfile(harnesses[0], harness_directory / harness_name)
    audit = release_tool.read_json(output / 'native-audit.json')
    runtimes = release_tool.read_json(output / 'runtime-inventory.json')['files']
    runtime_names = {item['name'] for item in runtimes}
    for name in runtime_names:
        require(re.fullmatch(r'[A-Za-z0-9._-]+', name) and sha(output / 'candidate' / name) == audit['files'][name], 'harness runtime is not the audited candidate file')
        shutil.copyfile(output / 'candidate' / name, harness_directory / name)
    if target['os'] != 'windows':
        (harness_directory / harness_name).chmod(0o700)
    if target['os'] == 'macos':
        # Relocate only owned harness copies. Installed candidate acceptance
        # remains independent and never inherits a loader-path override.
        from audit_native import relocate_macos, sign_macos
        relocation = relocate_macos(harness_directory, {harness_name} | runtime_names, runtime_names)
        signing, notarization = sign_macos(harness_directory, {harness_name} | runtime_names, None, None, output / 'evidence')
        require(all(item.get('verified') is True for item in signing['nested_code'].values()), 'portable harness ad-hoc code seals are unverified')
        write_json(output / 'evidence/harness-relocation.json', {'relocation': relocation, 'signing': signing, 'notarization': notarization})
    shutil.copyfile(harness_directory / harness_name, output / harness_name)
    harness_receipt = {'schema': 1, 'target': arguments.target, 'tag': arguments.tag, 'version': version, 'filename': harness_name, 'path': 'evidence/harness/' + harness_name, 'sha256': sha(output / harness_name), 'runtime_files': {name: sha(harness_directory / name) for name in sorted(runtime_names)}, 'source_commit': git_identity(root)}
    if target['os'] == 'windows':
        retained = output / 'evidence/windows-ort-build-receipt.json'
        harness_receipt['windows_ort_build_receipt'] = {'path': 'evidence/windows-ort-build-receipt.json', 'sha256': sha(retained)}
        retained_cache = output / 'evidence/windows-ort-CMakeCache.txt'
        harness_receipt['windows_ort_cmake_cache'] = validate_retained_windows_cmake(retained, retained_cache)
    harness_receipt['evidence_files'] = evidence_file_hashes(output)
    write_json(output / 'native-test-harness.json', harness_receipt)
    emit('result', command='native', state='passed', output=str(output), target=arguments.target)


def asset_hashes(files):
    hashes = {}
    for path in files:
        path = Path(path)
        require(path.is_file() and not path.is_symlink() and path.name not in hashes, 'duplicate or nonregular release asset')
        require(path.stat().st_size > 0, 'empty release asset')
        hashes[path.name] = sha(path)
    return hashes


def aggregate(arguments):
    targets = release_tool.load_targets(arguments.manifest)
    root = arguments.workspace.resolve().parent
    version = release_tool.workspace_version(arguments.workspace, arguments.tag)
    commit = git_identity(root)
    artifacts = arguments.artifacts.resolve()
    require({path.name for path in artifacts.iterdir()} == {'native-' + row['rust_target'] for row in targets}, 'candidate artifacts must contain exactly five native target directories')
    output = arguments.output.resolve()
    require(not output.exists(), 'aggregation output must be new')
    output.mkdir(parents=True)
    (output / 'audits').mkdir()
    archives, receipts = {}, {}
    for target in targets:
        native = artifacts / ('native-' + target['rust_target'])
        bridge_path = native / 'native-candidate-receipt.json'
        bridge = release_tool.read_json(bridge_path)
        require(bridge.get('schema') == 1 and bridge.get('state') == 'passed' and bridge.get('publication_allowed') is True and bridge.get('tag') == arguments.tag and bridge.get('target') == target['rust_target'], 'native candidate is unqualified or mismatched')
        require(native.is_dir() and not native.is_symlink(), 'native artifact directory must be regular')
        harness_name = 'native-test-binary.exe' if target['os'] == 'windows' else 'native-test-binary'
        expected_names = {'candidate', 'evidence', 'native-audit.json', 'runtime-inventory.json', 'dependency-inventory.json', 'embedding-command.json', 'embedding-receipt.json', target['archive'], 'SHA256SUMS', 'native-candidate-receipt.json', harness_name, 'native-test-harness.json'}
        require({path.name for path in native.iterdir()} == expected_names, 'native artifact contains missing or unexpected files')
        require(bridge.get('workspace_sha256') == sha(arguments.workspace) and bridge.get('lock_sha256') == sha(root / 'Cargo.lock'), 'native build source manifests differ')
        archive = native / target['archive']
        require(bridge['archive']['sha256'] == sha(archive), 'candidate archive differs from native receipt')
        audit_path = native / 'native-audit.json'
        for key, filename in (('runtime_inventory', 'runtime-inventory.json'), ('dependency_inventory', 'dependency-inventory.json'), ('embedding_receipt', 'embedding-receipt.json')):
            require(bridge.get(key, {}).get('sha256') == sha(native / filename), 'native bridge evidence bytes changed: ' + key)
        require(bridge.get('embedding_model_register_sha256') == sha(root / 'release/embedding-model.json'), 'native model register differs from candidate source')
        require(bridge['native_audit']['sha256'] == sha(audit_path), 'native audit differs from bridge receipt')
        audit = release_tool.audit_receipt(audit_path, target, version, arguments.tag)
        release_tool.verify_content(release_tool.read_archive(archive, target, audit), audit, version)
        require(bridge['files'] == audit['files'], 'candidate member hashes differ from native audit')
        harness = release_tool.read_json(native / 'native-test-harness.json')
        require(harness.get('filename') == harness_name and harness.get('source_commit') == commit and harness.get('target') == target['rust_target'] and harness.get('tag') == arguments.tag and harness.get('sha256') == sha(native / harness['filename']), 'native test harness identity mismatch')
        require(harness.get('evidence_files') == evidence_file_hashes(native), 'native evidence inventory differs from the exact harness receipt')
        embedding_spec_path = native / 'embedding-command.json'
        embedding_spec = release_tool.read_json(embedding_spec_path)
        embedding_wrapper = root / 'release/tests/embedding_acceptance.py'
        model_register = release_tool.read_json(root / 'release/embedding-model.json')
        model_files = model_register.get('files', {})
        require(embedding_spec.get('schema') == 1 and embedding_spec.get('state') == 'reviewed' and
                embedding_spec.get('protocol') == 'held-installed-process-v1' and
                embedding_spec.get('sha256') == sha(embedding_wrapper),
                'native embedding command is not bound to the reviewed source wrapper')
        require(isinstance(model_files, dict) and model_files and all(
                harness['evidence_files'].get('evidence/model/' + name) == digest
                for name, digest in model_files.items()),
                'native retained embedding model differs from the reviewed register')
        if target['os'] == 'windows':
            expected_binding = {'path': 'evidence/windows-ort-build-receipt.json',
                                'sha256': sha(native / 'evidence/windows-ort-build-receipt.json')}
            require(bridge.get('windows_ort_build_receipt') == expected_binding and harness.get('windows_ort_build_receipt') == expected_binding,
                    'Windows ORT source-build receipt custody differs')
            expected_cache = validate_retained_windows_cmake(
                native / expected_binding['path'], native / 'evidence/windows-ort-CMakeCache.txt')
            require(bridge.get('windows_ort_cmake_cache') == expected_cache and harness.get('windows_ort_cmake_cache') == expected_cache,
                    'Windows retained CMake cache custody differs')
            from audit_native import validate_windows_build_receipt
            source_receipt = release_tool.read_json(native / expected_binding['path'])
            provenance = validate_windows_build_receipt(
                source_receipt, release_tool.read_json(native / 'runtime-inventory.json')['files'], target['runner'])
            recorded = audit.get('windows_ort', {})
            require(recorded.get('build_receipt_sha256') == expected_binding['sha256'] and
                    all(recorded.get(key) == provenance.get(key) for key in
                        ('source_tag', 'source_commit', 'source_sha256', 'built_runtime_sha256', 'rust_crt', 'ort_crt')),
                    'Windows ORT source receipt differs from the native audit provenance')
        destination = output / target['archive']
        shutil.copyfile(archive, destination)
        archives[target['archive']] = sha(destination)
        suffix = '.exe' if target['os'] == 'windows' else ''
        receipts[target['rust_target']] = {'native_audit_sha256': sha(audit_path), 'candidate_receipt_sha256': sha(bridge_path), 'client_sha256': audit['files']['ilium' + suffix], 'server_sha256': audit['files']['ilium-server' + suffix], 'harness_sha256': harness['sha256'], 'native_test_harness_sha256': sha(native / 'native-test-harness.json'), 'evidence_files': harness['evidence_files'], 'evidence_files_sha256': evidence_files_digest(harness['evidence_files']), 'installed_embedding': {'wrapper_sha256': sha(embedding_wrapper), 'command_sha256': sha(embedding_spec_path), 'model_register_sha256': sha(root / 'release/embedding-model.json'), 'model_files': model_files, 'runtime_files': harness.get('runtime_files', {})}}
        shutil.copyfile(audit_path, output / 'audits' / (target['rust_target'] + '.json'))
    (output / 'SHA256SUMS').write_text(''.join(archives[name] + '  ' + name + '\n' for name in sorted(archives)), encoding='ascii')
    (output / 'VERSION').write_text(version + '\n', encoding='ascii')
    for name in ('install.sh', 'install.ps1'):
        shutil.copyfile(root / 'release' / name, output / name)
    source_inputs = {name: sha(root / name) for name in ('Cargo.toml', 'Cargo.lock', 'release/targets.toml', 'release/embedding-model.json', 'release/ort-source.json', 'release/ort-runtime.json', 'release/licence-sources.json')}
    metadata = {'schema': 1, 'source_inputs': source_inputs, 'tag': arguments.tag, 'commit': commit, 'archives': archives, 'target_receipts': receipts, 'installers': {name: sha(output / name) for name in ('install.sh', 'install.ps1')}}
    write_json(output / 'candidate.json', metadata)
    pages.build_pages(arguments.manifest, output / 'site', arguments.tag, output / 'SHA256SUMS', source_root=root)
    gh_output(subjects=json.dumps([str(output / name) for name in sorted(archives)]))
    emit('result', command='aggregate', state='passed', output=str(output), archives=archives, commit=commit)


def candidate_data(directory, manifest, workspace):
    directory = Path(directory)
    metadata = release_tool.read_json(directory / 'candidate.json')
    targets = release_tool.load_targets(manifest)
    version = release_tool.workspace_version(workspace, metadata['tag'])
    expected_inputs = {'Cargo.toml', 'Cargo.lock', 'release/targets.toml', 'release/embedding-model.json', 'release/ort-source.json', 'release/ort-runtime.json', 'release/licence-sources.json'}
    require(set(metadata.get('source_inputs', {})) == expected_inputs, 'candidate source inventory differs')
    for name, digest in metadata['source_inputs'].items():
        require(sha(Path(workspace).resolve().parent / name) == digest, 'candidate source input changed: ' + name)
    expected_names = set(metadata['archives']) | {'audits', 'site', 'SHA256SUMS', 'VERSION', 'install.sh', 'install.ps1', 'candidate.json'}
    actual_names = {path.name for path in directory.iterdir()}
    require(actual_names in (expected_names, expected_names | {'qualification.json'}), 'candidate aggregate file inventory differs')
    require(metadata.get('schema') == 1 and re.fullmatch('[0-9a-f]{40}', metadata.get('commit', '')), 'candidate source identity is invalid')
    require(set(metadata['archives']) == {row['archive'] for row in targets} and set(metadata['target_receipts']) == {row['rust_target'] for row in targets}, 'candidate target/archive inventory differs')
    require(set(metadata.get('installers', {})) == {'install.sh', 'install.ps1'}, 'candidate installer inventory differs')
    for name, digest in metadata['installers'].items():
        require(sha(directory / name) == digest, 'candidate installer bytes changed')
    for target in targets:
        archive = directory / target['archive']
        require(metadata['archives'][target['archive']] == sha(archive), 'candidate archive bytes changed')
        audit_path = directory / 'audits' / (target['rust_target'] + '.json')
        require(sha(audit_path) == metadata['target_receipts'][target['rust_target']]['native_audit_sha256'], 'candidate audit bytes changed')
        audit = release_tool.audit_receipt(audit_path, target, version, metadata['tag'])
        release_tool.verify_content(release_tool.read_archive(archive, target, audit), audit, version)
        release_tool.validate_checksums(directory / 'SHA256SUMS', targets, archive)
    pages.verify_pages(directory / 'site', directory / 'SHA256SUMS', source_root=Path(workspace).resolve().parent, manifest=manifest)
    return metadata, targets


def validate_install_receipt(metadata, proof, target, *, public):
    require(proof.get('schema') == 2 and proof.get('state') == 'passed' and proof.get('publication_allowed') is True, 'native installation did not pass')
    for key, expected in (('tag', metadata['tag']), ('target', target['rust_target']), ('archive_sha256', metadata['archives'][target['archive']]), ('installed_client_sha256', metadata['target_receipts'][target['rust_target']]['client_sha256']), ('installed_server_sha256', metadata['target_receipts'][target['rust_target']]['server_sha256'])):
        require(proof.get(key) == expected, 'qualification identity mismatch: ' + key)
    expected_system = {'linux': 'Linux', 'macos': 'Darwin', 'windows': 'Windows'}[target['os']]
    expected_machines = {'x86_64': {'x86_64', 'AMD64'}, 'aarch64': {'aarch64', 'arm64', 'ARM64'}}[target['arch']]
    identity = proof.get('native_identity')
    require(isinstance(identity, dict) and identity.get('system') == expected_system and
            identity.get('machine') in expected_machines and identity.get('runner') == target['runner'],
            'native installation identity differs from target runner/system/machine')
    suffix = '.exe' if target['os'] == 'windows' else ''
    expected_pair = {'ilium' + suffix: metadata['target_receipts'][target['rust_target']]['client_sha256'],
                     'ilium-server' + suffix: metadata['target_receipts'][target['rust_target']]['server_sha256']}
    require(proof.get('installed_pair_sha256') == expected_pair, 'installed pair digest map differs')
    require(proof.get('isolated_state_cleaned') is True, 'isolated owned installation was not cleaned')
    origin = proof.get('origin')
    source_mode = proof.get('installer_source_mode')
    if origin == 'local':
        require(not public and source_mode == ('exact-functions-with-local-download-adapter' if target['os'] == 'windows' else 'unmodified-installer'),
                'local installer source mode differs')
    else:
        require(origin == ORIGIN and public and source_mode in {'unmodified-installer', 'preview-no-argument-public-command', 'literal-no-argument-public-command'},
                'public installer origin/source mode differs')
    require(proof.get('public_transport_verified') is (origin == ORIGIN), 'public transport state differs from origin')
    tests = proof.get('pty_tests')
    require(isinstance(tests, list) and len(tests) == 2 and {row.get('name') for row in tests} == set(PTY_TESTS) and
            all(set(row) == {'name', 'command', 'exit_code', 'stdout', 'stderr'} and row.get('exit_code') == 0 and
                isinstance(row.get('command'), list) and len(row['command']) == 5 and
                row['command'][1:] == [row['name'], '--exact', '--nocapture', '--test-threads=1'] for row in tests),
            'missing or malformed substantive native PTY receipts')
    require(proof.get('native_test_binary_sha256') == metadata['target_receipts'][target['rust_target']]['harness_sha256'], 'native installation test harness digest differs')
    binding = metadata['target_receipts'][target['rust_target']].get('installed_embedding')
    embedding = proof.get('installed_embedding')
    require(isinstance(binding, dict) and isinstance(embedding, dict) and
            embedding.get('wrapper_sha256') == binding.get('wrapper_sha256') and
            embedding.get('command_sha256') == binding.get('command_sha256') and
            embedding.get('model_register_sha256') == binding.get('model_register_sha256') and
            embedding.get('model_files') == binding.get('model_files') and
            embedding.get('runtime_files') == binding.get('runtime_files') and
            embedding.get('binary_sha256') == proof.get('installed_client_sha256') and
            embedding.get('model_sha256') == binding.get('model_files', {}).get('model.onnx') and
            embedding.get('dimension') == 384 and embedding.get('finite_nonzero') is True and
            embedding.get('state') == 'passed', 'installed embedding proof is missing or not aggregate-bound')
    pair_directory = proof.get('installed_pair_directory')
    pair_path = PureWindowsPath(pair_directory) if target['os'] == 'windows' and isinstance(pair_directory, str) else Path(pair_directory) if isinstance(pair_directory, str) else None
    require(isinstance(pair_directory, str) and pair_path.is_absolute() and
            embedding.get('executable_path') == str(pair_path / ('ilium.exe' if target['os'] == 'windows' else 'ilium')) and
            type(embedding.get('process_id')) is int and embedding['process_id'] > 0,
            'installed embedding executable path or process identity differs')
    runtime = embedding.get('loaded_runtime')
    if runtime:
        runtime_path = PureWindowsPath(runtime) if target['os'] == 'windows' else Path(runtime)
        require(runtime_path.is_absolute() and runtime_path.parent == pair_path and
                binding.get('runtime_files', {}).get(runtime_path.name) == embedding.get('runtime_sha256'),
                'installed embedding runtime path/hash is not aggregate-bound')
    if target['os'] == 'macos':
        require(embedding.get('native_mapping_verified') is True and
                embedding.get('observed_with') == ['/bin/ps', '/usr/bin/vmmap'] and
                re.fullmatch(r'[0-9a-f]{64}', embedding.get('runtime_sha256', '')),
                'macOS installed embedding lacks independent native runtime mapping proof')
    elif target['os'] == 'windows':
        require(re.fullmatch(r'[0-9a-f]{64}', embedding.get('runtime_sha256', '')),
                'Windows installed embedding lacks shipped runtime identity')
    scenarios = proof.get('scenarios')
    required_scenarios = ({'upgrade', 'repeat', 'path_deduplication', 'pty', 'uninstall'} if public else
                          {'prior_fixture', 'corrupt_candidate_rollback', 'upgrade', 'repeat', 'path_deduplication', 'pty', 'uninstall'})
    require(isinstance(scenarios, dict) and set(scenarios) == required_scenarios and
            all(isinstance(value, dict) and value.get('state') == 'passed' for value in scenarios.values()),
            'transactional install scenario proof is missing or failed')
    reference_keys = {'path', 'sha256'}
    installer_keys = {'exit_code', 'stdout', 'stderr'}
    def installer_shape(value, *, succeeds):
        return (isinstance(value, dict) and set(value) == installer_keys and
                ((value.get('exit_code') == 0) if succeeds else (type(value.get('exit_code')) is int and value['exit_code'] != 0)) and
                all(isinstance(value.get(name), dict) and set(value[name]) == reference_keys for name in ('stdout', 'stderr')))
    if not public:
        prior = scenarios['prior_fixture']
        require(set(prior) == {'state', 'kind', 'fixture_version', 'candidate_version', 'installer', 'snapshot'} and
                prior['kind'] == 'task-owned-audited-bytes' and prior['fixture_version'] == '0.0.0-task7b-prior' and
                prior['candidate_version'] == metadata['tag'][1:] and installer_shape(prior['installer'], succeeds=True) and
                set(prior['snapshot']) == reference_keys,
                'prior fixture scenario schema differs')
        corrupt = scenarios['corrupt_candidate_rollback']
        require(set(corrupt) == {'state', 'installer', 'before', 'after'} and
                installer_shape(corrupt['installer'], succeeds=False) and
                set(corrupt['before']) == reference_keys and set(corrupt['after']) == reference_keys,
                'corrupt candidate scenario schema differs')
    upgrade = scenarios['upgrade']
    expected_upgrade = ({'state', 'installer', 'from', 'to'} if not public else {'state', 'scope', 'installer', 'to'})
    require(set(upgrade) == expected_upgrade and installer_shape(upgrade['installer'], succeeds=True) and
            upgrade['to'] == metadata['tag'][1:] and
            ((not public and upgrade['from'] == '0.0.0-task7b-prior') or
             (public and upgrade['scope'] == 'initial-clean-public-or-previous')), 'upgrade scenario schema differs')
    repeat = scenarios['repeat']
    require(set(repeat) == {'state', 'installer', 'before', 'after', 'version'} and
            installer_shape(repeat['installer'], succeeds=True) and repeat['version'] == metadata['tag'][1:] and
            set(repeat['before']) == reference_keys and set(repeat['after']) == reference_keys,
            'repeat scenario schema differs')
    require(set(scenarios['path_deduplication']) == {'state', 'evidence'} and
            set(scenarios['path_deduplication']['evidence']) == reference_keys, 'PATH scenario schema differs')
    require(set(scenarios['pty']) == {'state', 'tests'} and scenarios['pty']['tests'] == tests,
            'PTY scenario differs from exact test receipts')
    uninstall = scenarios['uninstall']
    require(set(uninstall) == {'state', 'installer', 'evidence'} and installer_shape(uninstall['installer'], succeeds=True) and
            set(uninstall['evidence']) == reference_keys, 'uninstall scenario schema differs')
    evidence_files = proof.get('evidence_files')
    require(isinstance(evidence_files, dict) and evidence_files and
            proof.get('evidence_files_sha256') == evidence_files_digest(evidence_files),
            'installation evidence inventory is missing or unbound')
    installer_name = 'install.ps1' if target['os'] == 'windows' else 'install.sh'
    require(proof.get('installer_sha256') == metadata['installers'][installer_name], 'installed source is not the qualified canonical installer')
    if public:
        require(proof.get('public_transport_verified') is True, 'qualification lacks real public HTTPS transport')


def validate_install_evidence(proof, receipt_path, target=None):
    evidence = Path(receipt_path).with_suffix('.evidence')
    require(evidence.is_dir() and not evidence.is_symlink(), 'native installation evidence is missing')
    actual = {}
    for path in sorted(evidence.rglob('*')):
        require(not path.is_symlink() and (path.is_dir() or path.is_file()), 'native installation evidence contains a link or special entry')
        if path.is_file():
            actual[path.relative_to(evidence).as_posix()] = sha(path)
    require(actual == proof.get('evidence_files') and evidence_files_digest(actual) == proof.get('evidence_files_sha256'), 'native installation evidence bytes differ from receipt')
    def evidence_path(reference):
        require(isinstance(reference, dict) and set(reference) == {'path', 'sha256'} and
                re.fullmatch(r'[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)*', reference.get('path', '')) and
                actual.get(reference['path']) == reference.get('sha256'), 'scenario evidence reference differs')
        return evidence / reference['path']
    def installer_evidence(value, *, succeeds, label):
        require(((value['exit_code'] == 0) if succeeds else value['exit_code'] != 0), 'installer exit code differs from scenario')
        stdout, stderr = evidence_path(value['stdout']), evidence_path(value['stderr'])
        require(stdout.name == label + '-stdout.txt' and stderr.name == label + '-stderr.txt', 'installer logs use unexpected paths')
        if label == 'corrupt-candidate':
            require('Archive SHA-256 mismatch' in stderr.read_text(encoding='utf-8'), 'corrupt-candidate did not fail closed on archive checksum mismatch')
    scenarios = proof['scenarios']
    public = proof.get('origin') != 'local'
    if not public:
        prior = json.loads(evidence_path(scenarios['prior_fixture']['snapshot']).read_text(encoding='utf-8'))
        corrupt_before = json.loads(evidence_path(scenarios['corrupt_candidate_rollback']['before']).read_text(encoding='utf-8'))
        corrupt_after = json.loads(evidence_path(scenarios['corrupt_candidate_rollback']['after']).read_text(encoding='utf-8'))
        require(prior == corrupt_before == corrupt_after and prior.get('schema') == 1 and
                prior.get('version') == scenarios['prior_fixture']['fixture_version'] and
                prior.get('pointer') == scenarios['prior_fixture']['fixture_version'] + '\n' and
                isinstance(prior.get('install_files'), dict) and prior['install_files'] and
                isinstance(prior.get('launcher_files'), dict) and prior['launcher_files'] and
                re.fullmatch(r'[0-9a-f]{64}', prior.get('path_state_sha256', '')),
                'corrupt candidate did not retain exact prior snapshot')
        installer_evidence(scenarios['prior_fixture']['installer'], succeeds=True, label='initial-install')
        installer_evidence(scenarios['corrupt_candidate_rollback']['installer'], succeeds=False, label='corrupt-candidate')
        installer_evidence(scenarios['upgrade']['installer'], succeeds=True, label='candidate-upgrade')
        require(scenarios['upgrade']['from'] == scenarios['prior_fixture']['fixture_version'], 'upgrade source version differs')
    else:
        installer_evidence(scenarios['upgrade']['installer'], succeeds=True, label='initial-install')
    repeat_before = json.loads(evidence_path(scenarios['repeat']['before']).read_text(encoding='utf-8'))
    repeat_after = json.loads(evidence_path(scenarios['repeat']['after']).read_text(encoding='utf-8'))
    require(repeat_before == repeat_after and repeat_before.get('schema') == 1 and
            repeat_before.get('version') == scenarios['repeat']['version'] and
            repeat_before.get('pointer') == scenarios['repeat']['version'] + '\n', 'repeat installation state differs')
    installer_evidence(scenarios['repeat']['installer'], succeeds=True, label='repeat-install')
    path_record = json.loads(evidence_path(scenarios['path_deduplication']['evidence']).read_text(encoding='utf-8'))
    require(set(path_record) == {'schema', 'kind', 'bin_directory', 'entry_count', 'original_sha256', 'installed_sha256', 'repeat_sha256', 'restored_sha256', 'exact_values'} and
            path_record['schema'] == 1 and path_record['kind'] in {'windows-user-registry', 'task-owned-posix-profile'} and
            Path(path_record['bin_directory']).is_absolute() and path_record['entry_count'] == 1 and
            path_record['installed_sha256'] == path_record['repeat_sha256'] and
            path_record['original_sha256'] != path_record['installed_sha256'] and
            path_record['original_sha256'] == path_record['restored_sha256'] and
            all(re.fullmatch(r'[0-9a-f]{64}', path_record[name]) for name in ('original_sha256', 'installed_sha256', 'repeat_sha256', 'restored_sha256')),
            'PATH/profile lifecycle evidence differs')
    state_digest = lambda value: release_tool.digest(json.dumps(value, sort_keys=True, separators=(',', ':')).encode())
    if path_record['kind'] == 'task-owned-posix-profile':
        values = path_record['exact_values']
        require(isinstance(values, dict) and set(values) == {'original', 'installed', 'repeat', 'restored'} and
                values['installed'] == values['repeat'] and values['original'] == values['restored'] and
                all(state_digest(values[name]) == path_record[name + '_sha256'] for name in values),
                'exact task-owned POSIX profile bytes differ')
    else:
        require(path_record['exact_values'] is None and
                state_digest(proof.get('windows_account_before', {}).get('user_path')) == path_record['original_sha256'] and
                state_digest(proof.get('windows_account_after', {}).get('user_path')) == path_record['installed_sha256'],
                'exact disposable Windows PATH snapshots differ')
    for test in proof['pty_tests']:
        stdout, stderr = evidence_path(test['stdout']), evidence_path(test['stderr'])
        require(stderr.is_file() and re.search(r'test result: ok\. 1 passed; 0 failed;', stdout.read_text(encoding='utf-8')), 'native PTY selected zero tests or failed')
    uninstall = json.loads(evidence_path(scenarios['uninstall']['evidence']).read_text(encoding='utf-8'))
    require(set(uninstall) == {'schema', 'pair_removed', 'launchers_removed', 'path_restored', 'sentinel_name', 'sentinel_sha256', 'sentinel_survived'} and
            uninstall.get('schema') == 1 and uninstall.get('pair_removed') is True and
            uninstall.get('launchers_removed') is True and uninstall.get('path_restored') is True and
            uninstall.get('sentinel_survived') is True and uninstall.get('sentinel_name') == 'task-owned-unrelated-sentinel.txt' and
            uninstall.get('sentinel_sha256') == release_tool.digest(b'must survive ownership-aware uninstall\n'),
            'ownership-aware uninstall evidence differs')
    installer_evidence(scenarios['uninstall']['installer'], succeeds=True, label='owned-uninstall')
    embedding = proof['installed_embedding']
    embedding_stdout = evidence_path(embedding['stdout'])
    embedding_stderr = evidence_path(embedding['stderr'])
    require(embedding_stdout.name == 'embedding-stdout.txt' and embedding_stderr.name == 'embedding-stderr.txt',
            'embedding logs use unexpected paths')
    records = [json.loads(line) for line in embedding_stdout.read_text(encoding='utf-8').splitlines()]
    raw = [record for record in records if record.get('type') == 'embedding-proof']
    require(len(raw) == 1 and raw[0].get('executable_path') == embedding.get('executable_path') and
            raw[0].get('ilium_pid') == embedding.get('process_id') and raw[0].get('binary_sha256') == embedding.get('binary_sha256') and
            raw[0].get('model_sha256') == embedding.get('model_sha256') and raw[0].get('loaded_runtime', '') == embedding.get('loaded_runtime', ''),
            'retained embedding JSON differs from receipt identities')
    vector = raw[0].get('embedding')
    require(isinstance(vector, list) and len(vector) == 384 and all(type(value) in (int, float) and math.isfinite(value) for value in vector) and
            any(value != 0 for value in vector) and
            release_tool.digest(json.dumps(vector, allow_nan=False).encode()) == embedding.get('vector_sha256'),
            'retained embedding vector differs or is not finite nonzero 384-dimensional')
    if target and target['os'] == 'macos':
        observed_path = evidence_path(embedding['observed_process'])
        mappings_path = evidence_path(embedding['native_mappings'])
        require(observed_path.name == 'embedding-observed_process.txt' and mappings_path.name == 'embedding-native_mappings.txt',
                'macOS native observation paths differ')
        observed = observed_path.read_text(encoding='utf-8')
        mappings = mappings_path.read_text(encoding='utf-8')
        require(re.search(re.escape(embedding['executable_path']) + r'(?=\s|$)', observed) and
                re.search(re.escape(embedding['loaded_runtime']) + r'(?=\s|$)', mappings),
                'retained macOS ps/vmmap bytes differ from installed executable/runtime')


def qualify_receipts(metadata, receipts, targets, *, public):
    require(set(receipts) == {row['rust_target'] for row in targets}, 'qualification requires all five native receipts')
    for target in targets:
        validate_install_receipt(metadata, receipts[target['rust_target']], target, public=public)
    return {'schema': 1, 'state': 'passed', 'publication_allowed': True, 'tag': metadata['tag'], 'commit': metadata['commit'], 'archives': metadata['archives'], 'installation_receipts': {target: release_tool.digest(json.dumps(proof, sort_keys=True).encode()) for target, proof in receipts.items()}, 'public_transport_verified': public}


def qualify(arguments):
    metadata, targets = candidate_data(arguments.candidate, arguments.manifest, arguments.workspace)
    receipts = {row['rust_target']: release_tool.read_json(arguments.receipts / (arguments.receipt_prefix + row['rust_target']) / 'receipt.json') for row in targets}
    expected_metadata = copy.deepcopy(metadata)
    if arguments.previous:
        require(arguments.baseline is not None, 'previous qualification requires channel baseline')
        baseline = load_baseline(arguments.baseline, metadata)
        require(set(receipts) == {row['rust_target'] for row in targets}, 'previous compatibility requires all five rows')
        if baseline['previous_production'] is None:
            for row in targets:
                proof = receipts[row['rust_target']]
                require(proof.get('state') == 'not-applicable' and proof.get('reason') == 'no previous production deployment' and proof.get('target') == row['rust_target'] and proof.get('commit') == metadata['commit'] and proof.get('tag') == metadata['tag'], 'first-release compatibility N/A is not evidence-bound')
            write_json(arguments.output, {'schema': 1, 'state': 'passed', 'compatibility': 'not-applicable: no previous production deployment', 'commit': metadata['commit'], 'tag': metadata['tag'], 'targets': [row['rust_target'] for row in targets]})
            emit('result', command='qualify', state='passed', output=str(arguments.output.resolve()))
            return
        expected_metadata['installers'] = {name: baseline['previous_production']['files'][name] for name in ('install.sh', 'install.ps1')}
    qualification = qualify_receipts(expected_metadata, receipts, targets, public=arguments.public)
    for row in targets:
        validate_install_evidence(receipts[row['rust_target']], arguments.receipts / (arguments.receipt_prefix + row['rust_target']) / 'receipt.json', row)
    if arguments.preview_tag:
        require(arguments.preview_receipt is not None, 'explicit preview qualification requires deployment receipt')
        preview = release_tool.read_json(arguments.preview_receipt)
        for row in targets:
            acquisition = release_tool.read_json(arguments.receipts / (arguments.receipt_prefix + row['rust_target']) / 'preview-acquisition.json')
            extension = 'ps1' if row['os'] == 'windows' else 'sh'
            require(acquisition.get('deployment_id') == preview['deployment_id'] and acquisition.get('url') == preview['url'].rstrip('/') + '/install.' + extension and acquisition.get('sha256') == metadata['installers']['install.' + extension] and acquisition.get('tag') == metadata['tag'] and acquisition.get('commit') == metadata['commit'], 'explicit preview source acquisition differs')
    if arguments.preview_receipt and not arguments.preview_tag:
        preview = release_tool.read_json(arguments.preview_receipt)
        require(preview.get('state') == 'passed' and preview.get('production') is False and preview.get('commit') == metadata['commit'] and preview.get('tag') == metadata['tag'], 'preview qualification receipt differs')
        for row in targets:
            extension = 'ps1' if row['os'] == 'windows' else 'sh'
            script_url = preview['url'].rstrip('/') + '/install.' + extension
            expected_command = (WINDOWS_COMMAND if row['os'] == 'windows' else POSIX_COMMAND).replace('https://ilium-setup.pages.dev/install.' + extension, script_url)
            require(receipts[row['rust_target']].get('pages_script_url') == script_url and receipts[row['rust_target']].get('preview_public_command') == expected_command and receipts[row['rust_target']].get('public_script_sha256') == metadata['installers']['install.' + extension], 'all-five preview defaults lack exact script/command identity')
    if arguments.literal:
        for row in targets:
            require(receipts[row['rust_target']].get('literal_public_command') == (WINDOWS_COMMAND if row['os'] == 'windows' else POSIX_COMMAND) and receipts[row['rust_target']].get('public_script_sha256') == metadata['installers']['install.ps1' if row['os'] == 'windows' else 'install.sh'], 'all-five final qualification requires literal public default commands and exact served script hashes')
    # This independent job reopens and audits all archives and all five native
    # receipts. No human approval or manually supplied success flag is accepted.
    write_json(arguments.output, qualification)
    emit('result', command='qualify', state='passed', output=str(arguments.output.resolve()), public_transport_verified=arguments.public)


def github(path, *, method='GET', payload=None, binary=None):
    token = os.environ.get('GH_TOKEN')
    require(bool(token), 'GitHub token is required for the explicit provider operation')
    url = path if path.startswith('https://') else 'https://api.github.com/repos/' + REPOSITORY + '/' + path
    require(urllib.parse.urlparse(url).netloc in {'api.github.com', 'uploads.github.com'}, 'unexpected GitHub API origin')
    headers = {'Accept': 'application/vnd.github+json', 'Authorization': 'Bearer ' + token, 'X-GitHub-Api-Version': '2022-11-28'}
    data = None
    if payload is not None:
        headers['Content-Type'] = 'application/json'; data = json.dumps(payload).encode()
    if binary is not None:
        headers['Content-Type'] = 'application/octet-stream'; data = Path(binary).read_bytes()
    content, _headers = request(url, method=method, data=data, headers=headers, limit=10_000_000)
    return json.loads(content)


def publication_files(candidate):
    candidate = Path(candidate)
    metadata = release_tool.read_json(candidate / 'candidate.json')
    return [candidate / name for name in sorted(metadata['archives'])] + [candidate / name for name in ('SHA256SUMS', 'VERSION', 'install.sh', 'install.ps1', 'candidate.json', 'qualification.json')]


def validate_release(release, tag, files, *, draft, immutable):
    require(release.get('tag_name') == tag and release.get('draft') is draft, 'release identity/draft state mismatch')
    if immutable:
        require(release.get('immutable') is True, 'published release is not immutable')
    assets = release.get('assets')
    require(isinstance(assets, list) and len(assets) == len(files), 'release asset count differs')
    inventory = {}
    for asset in assets:
        require(asset.get('name') not in inventory, 'duplicate GitHub release asset')
        require(isinstance(asset.get('size'), int) and asset['size'] > 0, 'empty GitHub release asset')
        inventory[asset.get('name')] = asset.get('digest')
    require(inventory == {name: 'sha256:' + digest for name, digest in files.items()}, 'GitHub release asset digest inventory differs')


def draft(arguments):
    metadata, _targets = candidate_data(arguments.candidate, arguments.manifest, arguments.workspace)
    qualification = release_tool.read_json(arguments.candidate / 'qualification.json')
    require(qualification.get('state') == 'passed' and qualification.get('commit') == metadata['commit'] and qualification.get('archives') == metadata['archives'], 'draft is not bound to native candidate qualification')
    # There is no overwrite/retry-by-replacement path. A pre-existing tag is
    # evidence requiring an explicit operator reconciliation outside this CLI.
    releases = github('releases?per_page=100')
    require(all(item.get('tag_name') != metadata['tag'] for item in releases), 'release tag already exists; replacement is forbidden')
    repository = github('immutable-releases')
    require(repository.get('enabled') is True, 'repository release immutability is not enabled')
    write_json(arguments.output.with_suffix('.intent.json'), {'schema': 1, 'operation': 'create-draft', 'commit': metadata['commit'], 'tag': metadata['tag'], 'owner_run_id': os.environ.get('GITHUB_RUN_ID'), 'assets': asset_hashes(publication_files(arguments.candidate))})
    release = github('releases', method='POST', payload={'tag_name': metadata['tag'], 'target_commitish': metadata['commit'], 'name': 'Ilium ' + metadata['tag'], 'body': 'Native five-target release; checksums, source-bound qualification and build attestations accompany these assets. See candidate.json and qualification.json for exact identities.', 'draft': True, 'prerelease': True, 'make_latest': 'false'})
    write_json(arguments.output.with_suffix('.owned.json'), {'schema': 1, 'owned_release_id': release['id'], 'commit': metadata['commit'], 'tag': metadata['tag'], 'owner_run_id': os.environ.get('GITHUB_RUN_ID')})
    upload = release['upload_url'].split('{', 1)[0]
    for file in publication_files(arguments.candidate):
        github(upload + '?name=' + urllib.parse.quote(file.name), method='POST', binary=file)
    files = asset_hashes(publication_files(arguments.candidate))
    readback = github('releases/' + str(release['id']))
    validate_release(readback, metadata['tag'], files, draft=True, immutable=False)
    write_json(arguments.output, {'schema': 1, 'state': 'passed', 'release_id': release['id'], 'tag': metadata['tag'], 'commit': metadata['commit'], 'assets': files, 'draft': True})
    gh_output(release_id=release['id'])
    emit('result', command='draft', state='passed', release_id=release['id'], output=str(arguments.output.resolve()))


def publish(arguments):
    metadata, _targets = candidate_data(arguments.candidate, arguments.manifest, arguments.workspace)
    qualification = release_tool.read_json(arguments.candidate / 'qualification.json')
    require(qualification.get('state') == 'passed' and qualification.get('publication_allowed') is True and qualification.get('commit') == metadata['commit'] and qualification.get('archives') == metadata['archives'], 'publication requires unchanged automatic native qualification')
    baseline = load_baseline(arguments.baseline, metadata)
    assert_latest(baseline); assert_production(baseline)
    draft_receipt = release_tool.read_json(arguments.draft_receipt)
    files = asset_hashes(publication_files(arguments.candidate))
    require(draft_receipt.get('tag') == metadata['tag'] and draft_receipt.get('commit') == metadata['commit'] and draft_receipt.get('assets') == files, 'draft bytes no longer match qualification')
    release_id = draft_receipt['release_id']
    validate_release(github('releases/' + str(release_id)), metadata['tag'], files, draft=True, immutable=False)
    write_json(arguments.output.with_suffix('.intent.json'), {'schema': 1, 'operation': 'publish-prerelease', 'owned_release_id': release_id, 'commit': metadata['commit'], 'tag': metadata['tag'], 'assets': files, 'request': {'draft': False, 'prerelease': True, 'make_latest': 'false'}})
    github('releases/' + str(release_id), method='PATCH', payload={'draft': False, 'prerelease': True, 'make_latest': 'false'})
    published = github('releases/' + str(release_id))
    assert_latest(baseline); assert_production(baseline)
    require(published.get('prerelease') is True, 'candidate publication must remain a prerelease before stable activation')
    validate_release(published, metadata['tag'], files, draft=False, immutable=True)
    write_json(arguments.output, {'schema': 1, 'state': 'passed', 'release_id': release_id, 'tag': metadata['tag'], 'commit': metadata['commit'], 'assets': files, 'immutable': True, 'url': published['html_url']})
    gh_output(release_id=release_id)
    emit('result', command='publish', state='passed', release_id=release_id, url=published['html_url'], output=str(arguments.output.resolve()))


def latest(arguments):
    metadata, targets = candidate_data(arguments.candidate, arguments.manifest, arguments.workspace)
    receipts = {row['rust_target']: release_tool.read_json(arguments.receipts / (arguments.receipt_prefix + row['rust_target']) / 'receipt.json') for row in targets}
    qualify_receipts(metadata, receipts, targets, public=True)
    for row in targets:
        validate_install_evidence(receipts[row['rust_target']], arguments.receipts / (arguments.receipt_prefix + row['rust_target']) / 'receipt.json', row)
    publication = release_tool.read_json(arguments.publication_receipt)
    files = asset_hashes(publication_files(arguments.candidate))
    require(publication.get('assets') == files and publication.get('commit') == metadata['commit'], 'latest candidate differs from published candidate')
    release_id = publication['release_id']
    validate_release(github('releases/' + str(release_id)), metadata['tag'], files, draft=False, immutable=True)
    baseline = load_baseline(arguments.baseline, metadata)
    ready = release_tool.read_json(arguments.recovery_ready / 'recovery-ready.json')
    require(ready.get('state') == 'passed' and ready.get('commit') == metadata['commit'] and ready.get('tag') == metadata['tag'], 'promotion lacks prepared source-bound recovery')
    assert_latest(baseline); assert_production(baseline)
    write_json(arguments.output.with_suffix('.intent.json'), {'schema': 1, 'operation': 'promote-latest', 'owned_release_id': release_id, 'commit': metadata['commit'], 'tag': metadata['tag'], 'assets': files, 'baseline': baseline, 'request': {'prerelease': False, 'make_latest': 'true'}})
    github('releases/' + str(release_id), method='PATCH', payload={'prerelease': False, 'make_latest': 'true'})
    readback = github('releases/latest')
    require(readback.get('prerelease') is False, 'stable activation did not clear prerelease')
    validate_release(readback, metadata['tag'], files, draft=False, immutable=True)
    require(public_latest_tag() == metadata['tag'], 'public latest redirect did not activate candidate')
    write_json(arguments.output, {'schema': 1, 'state': 'passed', 'tag': metadata['tag'], 'release_id': release_id, 'previous_latest': baseline['previous_latest'], 'timing': 'after five pinned GitHub installs, before literal default public installs; a default latest installer cannot be tested before activation'})
    emit('result', command='latest', state='passed', output=str(arguments.output.resolve()))


def hosted_bytes(site, origin):
    require(re.fullmatch(r'https://[a-z0-9.-]+\.pages\.dev', origin), 'unexpected Pages readback origin')
    site = Path(site)
    hashes = asset_hashes([site / name for name in pages.FILES - {'_headers'}])
    for name, expected in hashes.items():
        if name == '404.html':
            continue
        path = '/' if name == 'index.html' else '/' + name
        content, headers = request(origin + path, headers={'Cache-Control': 'no-cache'}, limit=2_000_000)
        require(release_tool.digest(content) == expected, 'hosted Pages bytes differ: ' + name)
        require('no-store' in headers.get('Cache-Control', '').lower() and headers.get('X-Content-Type-Options') == 'nosniff', 'hosted Pages cache/security header mismatch')
        require(headers.get('Content-Security-Policy') == "default-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'", 'hosted Pages CSP differs')
        if name.endswith(('.sh', '.ps1')):
            require(headers.get_content_type() == 'text/plain', 'hosted installer MIME differs')
        if name == 'manifest.json':
            require(headers.get_content_type() == 'application/json', 'hosted manifest MIME differs')
    # request() normally sanitizes HTTP errors. Inspect this known route directly
    # to retain its status/body as custom404 evidence without exposing API data.
    try:
        urllib.request.build_opener(HTTPSOnly()).open(origin + '/ilium-resource-that-must-not-exist', timeout=30)
    except urllib.error.HTTPError as error:
        require(error.code == 404 and release_tool.digest(error.read(2_000_001)) == hashes['404.html'], 'Pages missing route is not the exact custom 404')
        error.close()
    else:
        raise release_tool.ReleaseError('Pages unexpectedly uses an application fallback')
    return hashes


def cloudflare(path, *, method='GET'):
    token = os.environ.get('CLOUDFLARE_API_TOKEN')
    require(bool(token), 'Cloudflare token is required for explicit deployment')
    content, _headers = request('https://api.cloudflare.com/client/v4/' + path, method=method, headers={'Authorization': 'Bearer ' + token}, limit=10_000_000)
    response = json.loads(content)
    require(response.get('success') is True, 'Cloudflare API returned failure')
    return response['result']


def deploy(arguments):
    metadata, _targets = candidate_data(arguments.candidate, arguments.manifest, arguments.workspace)
    baseline = load_baseline(arguments.baseline, metadata)
    assert_production(baseline)
    publication = release_tool.read_json(arguments.publication_receipt)
    if arguments.production:
        actual = latest_identity()
        require(actual and actual['id'] == publication['release_id'] and actual['tag'] == metadata['tag'], 'production deployment requires this run to own latest')
    else:
        assert_latest(baseline)
    branch = baseline['production_branch'] if arguments.production else 'release-' + metadata['tag'] + '-' + os.environ['GITHUB_RUN_ID'] + '-' + os.environ.get('GITHUB_RUN_ATTEMPT', '1')
    record = upload_pages(arguments.candidate / 'site', branch, metadata['commit'], arguments.log)
    require(record.get('environment') == ('production' if arguments.production else 'preview'), 'Pages deployment environment differs')
    hashes = hosted_bytes(arguments.candidate / 'site', record['url'].rstrip('/'))
    prefix, project = pages_project()
    if arguments.production:
        require(project.get('canonical_deployment', {}).get('id') == record['id'], 'Pages canonical production ID differs from owned deployment')
        hosted_bytes(arguments.candidate / 'site', 'https://' + pages.HOST)
    else:
        assert_production(baseline)
    write_json(arguments.output, {'schema': 1, 'state': 'passed', 'tag': metadata['tag'], 'commit': metadata['commit'], 'deployment_id': record['id'], 'url': record['url'], 'production': arguments.production, 'host': pages.HOST, 'files': hashes})
    emit('result', command='deploy', state='passed', output=str(arguments.output.resolve()), deployment_id=record['id'], url=record['url'])


def install(arguments):
    metadata, targets = candidate_data(arguments.candidate, arguments.manifest, arguments.workspace)
    target = release_tool.selected_target(arguments.manifest, arguments.target)
    root = arguments.workspace.resolve().parent
    if arguments.mode == 'previous':
        require(arguments.baseline is not None, 'previous source compatibility requires retained baseline')
        baseline = load_baseline(arguments.baseline, metadata)
        if baseline['previous_production'] is None:
            write_json(arguments.output, {'schema': 1, 'state': 'not-applicable', 'reason': 'no previous production deployment', 'tag': metadata['tag'], 'commit': metadata['commit'], 'target': target['rust_target']})
            emit('result', command='install', mode='previous', state='not-applicable', target=target['rust_target'], output=str(arguments.output.resolve()))
            return
    native = arguments.native.resolve()
    expected = metadata['target_receipts'][target['rust_target']]
    harness_path = native / 'native-test-harness.json'
    require(expected.get('native_test_harness_sha256') == sha(harness_path),
            'native test harness receipt differs from aggregate-bound bytes')
    harness = release_tool.read_json(harness_path)
    require(harness.get('evidence_files') == expected.get('evidence_files') and
            expected.get('evidence_files_sha256') == evidence_files_digest(harness.get('evidence_files')),
            'native evidence inventory differs from aggregate-bound values')
    require(harness.get('source_commit') == metadata['commit'] and harness.get('target') == target['rust_target'] and harness.get('sha256') == sha(native / harness['filename']), 'install harness is not source-bound')
    require(harness.get('path') == 'evidence/harness/' + harness['filename'], 'native harness path is not the owned evidence directory')
    harness_binary = native / harness['path']
    require(sha(harness_binary) == harness['sha256'], 'portable native harness bytes differ')
    for name, digest in harness.get('runtime_files', {}).items():
        require(re.fullmatch(r'[A-Za-z0-9._-]+', name) and sha(harness_binary.parent / name) == digest, 'portable harness runtime bytes changed')
    require(harness.get('evidence_files') == evidence_file_hashes(native), 'portable native evidence bytes changed')
    if target['os'] == 'windows':
        receipt_binding = harness.get('windows_ort_build_receipt', {})
        cache_binding = harness.get('windows_ort_cmake_cache', {})
        require(expected['evidence_files'].get(receipt_binding.get('path')) == receipt_binding.get('sha256') and
                expected['evidence_files'].get(cache_binding.get('path')) == cache_binding.get('sha256'),
                'Windows source-build evidence differs from aggregate-bound values')
        require(validate_retained_windows_cmake(native / receipt_binding['path'], native / cache_binding['path']) == cache_binding,
                'Windows retained CMake cache differs during install')
    if target['os'] != 'windows':
        harness_binary.chmod(0o700)
    installer = root / 'release' / ('install.ps1' if target['os'] == 'windows' else 'install.sh')
    if arguments.mode == 'previous':
        installer = arguments.baseline / 'previous' / installer.name
    origin = 'local' if arguments.mode == 'candidate' else ORIGIN
    proof = arguments.output.resolve()
    if arguments.mode == 'preview-tag':
        require(arguments.preview_receipt is not None, 'explicit preview install requires source-bound deployment receipt')
        preview = release_tool.read_json(arguments.preview_receipt)
        require(preview.get('state') == 'passed' and preview.get('production') is False and preview.get('commit') == metadata['commit'] and preview.get('tag') == metadata['tag'], 'preview receipt is not source-bound')
        script_url = preview['url'].rstrip('/') + '/install.' + ('ps1' if target['os'] == 'windows' else 'sh')
        content, _headers = request(script_url, limit=2_000_000)
        require(release_tool.digest(content) == metadata['installers'][installer.name], 'explicit preview source differs from frozen installer')
        proof.parent.mkdir(parents=True, exist_ok=True)
        installer = proof.parent / ('preview-' + installer.name)
        with installer.open('xb') as script:
            script.write(content)
        write_json(proof.parent / 'preview-acquisition.json', {'schema': 1, 'url': script_url, 'sha256': release_tool.digest(content), 'deployment_id': preview['deployment_id'], 'tag': metadata['tag'], 'commit': metadata['commit']})
    command = [sys.executable, root / 'release/tests/native_install.py', '--manifest', arguments.manifest.resolve(), '--installer', installer, '--archive-directory', arguments.candidate.resolve(), '--origin', origin, '--tag', metadata['tag'], '--target', target['rust_target'], '--runner-identity', target['runner'], '--expected-client-sha256', expected['client_sha256'], '--expected-server-sha256', expected['server_sha256'], '--native-test-binary', harness_binary, '--output', proof]
    embedding = expected['installed_embedding']
    command.extend(['--embedding-wrapper', root / 'release/tests/embedding_acceptance.py',
                    '--embedding-command', native / 'embedding-command.json',
                    '--embedding-model', native / 'evidence/model/model.onnx',
                    '--embedding-model-register', root / 'release/embedding-model.json',
                    '--expected-embedding-wrapper-sha256', embedding['wrapper_sha256'],
                    '--expected-embedding-command-sha256', embedding['command_sha256'],
                    '--expected-embedding-model-register-sha256', embedding['model_register_sha256'],
                    '--expected-embedding-model-files', json.dumps(embedding['model_files'], sort_keys=True, separators=(',', ':')),
                    '--expected-embedding-runtime-files', json.dumps(embedding['runtime_files'], sort_keys=True, separators=(',', ':'))])
    if arguments.mode == 'preview':
        require(arguments.preview_receipt is not None, 'preview install requires source-bound Pages deployment receipt')
        preview = release_tool.read_json(arguments.preview_receipt)
        require(preview.get('state') == 'passed' and preview.get('production') is False and preview.get('commit') == metadata['commit'] and preview.get('tag') == metadata['tag'], 'preview receipt is not source-bound')
        script_url = preview['url'].rstrip('/') + '/install.' + ('ps1' if target['os'] == 'windows' else 'sh')
        command.extend(['--pages-script-url', script_url])
        hosted_bytes(arguments.candidate / 'site', preview['url'].rstrip('/'))
    if arguments.mode == 'public':
        # Public default acquisition must execute the literal, unmodified command.
        # native_install owns the isolated installation and subsequent real PTY
        # acceptance; this flag is a distinct contract, never a pinned substitute.
        command.extend(['--literal-public-command', WINDOWS_COMMAND if target['os'] == 'windows' else POSIX_COMMAND])
        hosted_bytes(arguments.candidate / 'site', 'https://' + pages.HOST)
    logged(command, root, arguments.log)
    receipt = release_tool.read_json(proof)
    expected_metadata = copy.deepcopy(metadata)
    if arguments.mode == 'previous':
        expected_metadata['installers'] = {name: baseline['previous_production']['files'][name] for name in ('install.sh', 'install.ps1')}
    validate_install_receipt(expected_metadata, receipt, target, public=arguments.mode != 'candidate')
    validate_install_evidence(receipt, proof, target)
    if arguments.mode == 'public':
        require(receipt.get('literal_public_command') == (WINDOWS_COMMAND if target['os'] == 'windows' else POSIX_COMMAND), 'native receipt does not prove the literal public default command')
    emit('result', command='install', mode=arguments.mode, state='passed', target=arguments.target, output=str(proof))

class HTTPFailure(release_tool.ReleaseError):
    def __init__(self, status):
        super().__init__('HTTPS request failed with status ' + str(status))
        self.status = status


def public_latest_tag():
    try:
        with urllib.request.build_opener(HTTPSOnly()).open(ORIGIN + '/latest', timeout=60) as response:
            final = response.geturl()
    except urllib.error.HTTPError as error:
        if error.code == 404:
            error.close(); return None
        raise HTTPFailure(error.code) from None
    prefix = ORIGIN + '/tag/'
    require(final.startswith(prefix) and re.fullmatch(pages.TAG_PATTERN, final[len(prefix):]), 'public latest redirect does not identify one safe release tag')
    return final[len(prefix):]


def latest_identity():
    try:
        record = github('releases/latest')
    except HTTPFailure as error:
        if error.status != 404:
            raise
        require(public_latest_tag() is None, 'latest API absence and public redirect disagree')
        return None
    require(not record.get('draft') and not record.get('prerelease') and record.get('immutable') is True, 'baseline latest must be a published immutable full release')
    require(public_latest_tag() == record['tag_name'], 'API latest and public latest redirect disagree')
    hashes = {item['name']: item.get('digest') for item in record['assets']}
    require(len(hashes) == len(record['assets']) and all(isinstance(value, str) and re.fullmatch('sha256:[0-9a-f]{64}', value) for value in hashes.values()), 'baseline latest asset inventory lacks exact digests')
    return {'id': record['id'], 'tag': record['tag_name'], 'assets': hashes}


def pages_project():
    project = os.environ.get('CLOUDFLARE_PAGES_PROJECT')
    account = os.environ.get('CLOUDFLARE_ACCOUNT_ID')
    require(project == 'ilium-setup' and account and re.fullmatch('[0-9a-f]{32}', account), 'Cloudflare account/project variables are not configured')
    prefix = 'accounts/' + account + '/pages/projects/' + project
    record = cloudflare(prefix)
    require(record.get('subdomain') == pages.HOST and not record.get('source') and record.get('production_branch') == 'master', 'Pages project identity/direct-upload mode differs')
    return prefix, record


def missing_response(url):
    try:
        with urllib.request.build_opener(HTTPSOnly()).open(url, timeout=30):
            raise release_tool.ReleaseError('expected absent endpoint is publicly available')
    except urllib.error.HTTPError as error:
        require(error.code == 404, 'absence must be HTTP 404, not a provider/authentication error')
        content = error.read(2_000_001)
        require(len(content) <= 2_000_000, 'oversized missing-resource response')
        headers = error.headers
        error.close()
        return content, headers


def verify_absent_installers(origin):
    for route in ('/install.sh', '/install.ps1', '/manifest.json'):
        missing_response(origin.rstrip('/') + route)


def verify_baseline_bytes(baseline):
    prior = baseline['previous_production']
    if prior is None:
        verify_absent_installers('https://' + pages.HOST)
        return
    for name, digest in prior['files'].items():
        if name == '404.html':
            body, headers = missing_response('https://' + pages.HOST + '/ilium-resource-that-must-not-exist')
        else:
            body, headers = request('https://' + pages.HOST + ('/' if name == 'index.html' else '/' + name), limit=2_000_000)
        require(release_tool.digest(body) == digest, 'restored production byte identity differs from baseline')
        require('no-store' in headers.get('Cache-Control', '').lower() and headers.get('X-Content-Type-Options') == 'nosniff', 'restored production cache/security headers differ')
        if name.endswith(('.sh', '.ps1')):
            require(headers.get_content_type() == 'text/plain', 'restored installer MIME differs')


def capture_baseline(arguments):
    output = arguments.output.resolve()
    require(not output.exists(), 'channel baseline output must be new')
    latest = latest_identity()
    _prefix, project = pages_project()
    production = project.get('canonical_deployment')
    if production is None:
        verify_absent_installers('https://' + pages.HOST)
    if production is not None:
        require(production.get('environment') == 'production' and production.get('latest_stage', {}).get('status') == 'success', 'previous Pages deployment is not a successful production rollback target')
    output.mkdir(parents=True)
    baseline = {'schema': 1, 'state': 'passed', 'commit': git_identity(arguments.workspace.resolve().parent), 'tag': 'v' + workspace_version(arguments.workspace), 'previous_latest': latest, 'previous_production': None, 'production_branch': project['production_branch'], 'host': pages.HOST}
    if production:
        previous = output / 'previous'; previous.mkdir()
        content, _headers = request('https://' + pages.HOST + '/manifest.json', limit=2_000_000)
        metadata = json.loads(content)
        require(metadata.get('host') == pages.HOST and set(metadata.get('files', {})) == pages.FILES - {'manifest.json'}, 'previous production manifest inventory differs')
        require(latest is not None and metadata.get('release_tag') == latest['tag'], 'previous production and latest identities disagree')
        (previous / 'manifest.json').write_bytes(content)
        hashes = {'manifest.json': release_tool.digest(content)}
        for name in ('index.html', 'install.sh', 'install.ps1'):
            url = 'https://' + pages.HOST + ('/' if name == 'index.html' else '/' + name)
            body, headers = request(url, limit=2_000_000)
            pages.text_content(body)
            require(release_tool.digest(body) == metadata['files'][name], 'previous public bytes differ from previous deployment manifest')
            require('no-store' in headers.get('Cache-Control', '').lower(), 'previous installer/site cache policy is not safe for channel recovery')
            (previous / name).write_bytes(body)
            hashes[name] = release_tool.digest(body)
        missing_body, missing_headers = missing_response('https://' + pages.HOST + '/ilium-resource-that-must-not-exist')
        require(release_tool.digest(missing_body) == metadata['files']['404.html'], 'previous custom 404 differs from deployment manifest')
        (previous / '404.html').write_bytes(missing_body)
        hashes['404.html'] = release_tool.digest(missing_body)
        baseline['previous_production'] = {'id': production['id'], 'url': production['url'], 'files': hashes}
    write_json(output / 'baseline.json', baseline)
    emit('result', command='baseline', state='passed', output=str(output), first_release=latest is None)


def load_baseline(path, metadata=None):
    path = Path(path)
    baseline = release_tool.read_json(path / 'baseline.json')
    require(baseline.get('schema') == 1 and baseline.get('state') == 'passed' and baseline.get('host') == pages.HOST and baseline.get('production_branch') == 'master', 'invalid channel baseline')
    if metadata is not None:
        require(baseline.get('commit') == metadata['commit'] and baseline.get('tag') == metadata['tag'], 'channel baseline belongs to another source')
    previous = baseline.get('previous_production')
    if previous:
        for name, digest in previous['files'].items():
            require(name in {'index.html', 'install.sh', 'install.ps1', 'manifest.json', '404.html'} and sha(path / 'previous' / name) == digest, 'retained previous production bytes changed')
    return baseline


def assert_latest(baseline, candidate=None):
    actual = latest_identity()
    expected = candidate if candidate is not None else baseline['previous_latest']
    require(actual == expected, 'latest channel changed ownership or byte inventory')


def assert_production(baseline):
    _prefix, project = pages_project()
    current = project.get('canonical_deployment')
    require((current or {}).get('id') == (baseline.get('previous_production') or {}).get('id'), 'production Pages channel changed ownership')


def withdrawal_files():
    # Complete reviewed proposal bytes, embedded so recovery never depends on
    # the advisor's scratch directory or a mutable remote artifact.
    return {
        'index.html': '''<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Ilium installation unavailable</title>
</head>
<body>
<main>
<h1>Ilium installation is temporarily unavailable</h1>
<p>The public installation channel has been withdrawn while release verification is completed.</p>
<p>Installer endpoints are unavailable. Existing installations and running sessions remain unchanged.</p>
</main>
</body>
</html>
'''.encode(),
        '404.html': '''<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Installer unavailable</title>
</head>
<body>
<main>
<h1>Installer unavailable</h1>
<p>The public installation channel is temporarily unavailable.</p>
<p><a href="/">Installation status</a></p>
</main>
</body>
</html>
'''.encode(),
        '_headers': '''/*
  Cache-Control: no-store, max-age=0
  X-Content-Type-Options: nosniff
  Referrer-Policy: no-referrer
  Content-Security-Policy: default-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'
  X-Frame-Options: DENY
'''.encode(),
    }


def upload_pages(directory, branch, commit, log):
    marker = 'ilium-run:' + os.environ['GITHUB_RUN_ID'] + ':' + os.environ.get('GITHUB_RUN_ATTEMPT', '1') + ':' + uuid.uuid4().hex
    log = Path(log)
    write_json(log.with_suffix('.intent.json'), {'schema': 1, 'operation': 'pages-deploy', 'commit': commit, 'branch': branch, 'owner_marker': marker, 'files': asset_hashes(list(Path(directory).iterdir()))})
    logged(['npx', '--yes', WRANGLER, 'pages', 'deploy', str(Path(directory).resolve()), '--project-name', 'ilium-setup', '--branch', branch, '--commit-hash', commit, '--commit-message', marker], ROOT, log)
    prefix, _project = pages_project()
    deployments = cloudflare(prefix + '/deployments?per_page=100')
    matches = [item for item in deployments if item.get('latest_stage', {}).get('status') == 'success' and item.get('deployment_trigger', {}).get('metadata', {}).get('branch') == branch and item.get('deployment_trigger', {}).get('metadata', {}).get('commit_hash') == commit and item.get('deployment_trigger', {}).get('metadata', {}).get('commit_message') == marker]
    require(len(matches) == 1, 'deployment readback must identify one exact owned marker/branch/source attempt')
    record = matches[0]
    write_json(log.with_suffix('.readback.json'), {'schema': 1, 'deployment_id': record['id'], 'url': record['url'], 'environment': record['environment'], 'commit': commit, 'owner_marker': marker})
    return record


def verify_withdrawal(directory, origin):
    files = withdrawal_files()
    require({path.name for path in Path(directory).iterdir()} == set(files), 'withdrawal artifact inventory differs')
    for name, content in files.items():
        require((Path(directory) / name).read_bytes() == content, 'withdrawal artifact differs from reviewed bytes')
    body, headers = request(origin.rstrip('/') + '/', limit=2_000_000)
    require(body == files['index.html'] and 'no-store' in headers.get('Cache-Control', '').lower(), 'withdrawal landing page bytes/cache differ')
    for route in ('/install.sh', '/install.ps1', '/manifest.json', '/ilium-missing-resource'):
        try:
            with urllib.request.build_opener(HTTPSOnly()).open(origin.rstrip('/') + route, timeout=30):
                raise release_tool.ReleaseError('withdrawal still exposes an installer or manifest')
        except urllib.error.HTTPError as error:
            require(error.code == 404 and error.read(2_000_001) == files['404.html'], 'withdrawal missing-route bytes/status differ')
            require('no-store' in error.headers.get('Cache-Control', '').lower() and error.headers.get('X-Content-Type-Options') == 'nosniff', 'withdrawal missing-route security/cache differs')
            error.close()
    return {name: release_tool.digest(content) for name, content in files.items()}


def recovery_ready(arguments):
    metadata, _targets = candidate_data(arguments.candidate, arguments.manifest, arguments.workspace)
    baseline = load_baseline(arguments.baseline, metadata)
    assert_latest(baseline); assert_production(baseline)
    output = arguments.output.resolve()
    require(not output.exists(), 'prepared recovery output must be new')
    output.mkdir(parents=True)
    receipt = {'schema': 1, 'state': 'passed', 'commit': metadata['commit'], 'tag': metadata['tag'], 'previous_production': baseline['previous_production'], 'withdrawal': None}
    if baseline['previous_production'] is not None:
        prefix, _project = pages_project()
        previous = cloudflare(prefix + '/deployments/' + baseline['previous_production']['id'])
        require(previous.get('environment') == 'production' and previous.get('latest_stage', {}).get('status') == 'success', 'saved production is not a successful rollback target')
    if baseline['previous_production'] is None:
        directory = output / 'withdrawal'; directory.mkdir()
        for name, content in withdrawal_files().items():
            (directory / name).write_bytes(content)
        branch = 'withdrawal-' + os.environ['GITHUB_RUN_ID'] + '-' + os.environ.get('GITHUB_RUN_ATTEMPT', '1')
        deployment = upload_pages(directory, branch, metadata['commit'], arguments.log)
        require(deployment.get('environment') == 'preview', 'withdrawal preparation must not change production')
        hashes = verify_withdrawal(directory, deployment['url'])
        assert_production(baseline)
        receipt['withdrawal'] = {'deployment_id': deployment['id'], 'url': deployment['url'], 'files': hashes}
    write_json(output / 'recovery-ready.json', receipt)
    emit('result', command='recovery-ready', state='passed', output=str(output))


def readback(arguments):
    metadata, _targets = candidate_data(arguments.candidate, arguments.manifest, arguments.workspace)
    publication = release_tool.read_json(arguments.publication_receipt)
    files = asset_hashes(publication_files(arguments.candidate))
    latest = github('releases/latest')
    require(latest.get('id') == publication['release_id'] and latest.get('prerelease') is False, 'final latest no longer belongs to this release')
    validate_release(latest, metadata['tag'], files, draft=False, immutable=True)
    require(public_latest_tag() == metadata['tag'], 'final public latest redirect differs')
    production = release_tool.read_json(arguments.production_receipt)
    _prefix, project = pages_project()
    require(project.get('canonical_deployment', {}).get('id') == production['deployment_id'], 'final production deployment changed ownership')
    hashes = hosted_bytes(arguments.candidate / 'site', 'https://' + pages.HOST)
    write_json(arguments.output, {'schema': 1, 'state': 'passed', 'tag': metadata['tag'], 'commit': metadata['commit'], 'latest_release_id': latest['id'], 'production_deployment_id': production['deployment_id'], 'public_files': hashes, 'immutable': True})
    emit('result', command='readback', state='passed', output=str(arguments.output.resolve()))


def recovery_write(arguments, operation, invoke, readback, verify):
    """Reconcile an uncertain owned write from readback without resubmitting."""
    stem = arguments.output.parent / (operation + '-' + uuid.uuid4().hex)
    write_json(stem.with_suffix('.intent.json'), {'schema': 1, 'operation': operation, 'phase': 'before-request'})
    failure = None
    try:
        invoke()
        write_json(stem.with_suffix('.response.json'), {'schema': 1, 'operation': operation, 'phase': 'response', 'state': 'returned'})
    except (ValueError, OSError) as error:
        failure = str(error)[:1000]
        write_json(stem.with_suffix('.response.json'), {'schema': 1, 'operation': operation, 'phase': 'response', 'state': 'outcome-unknown', 'error': failure})
    actual = readback()
    require(verify(actual), operation + ' authoritative readback does not establish intended recovery')
    write_json(stem.with_suffix('.readback.json'), {'schema': 1, 'operation': operation, 'phase': 'readback', 'state': 'verified', 'reconciled_uncertain_response': failure is not None})
    return actual


def recover(arguments):
    metadata, _targets = candidate_data(arguments.candidate, arguments.manifest, arguments.workspace)
    baseline = load_baseline(arguments.baseline, metadata)
    publication_path = arguments.publication_receipt
    if not publication_path.exists():
        publication_path = publication_path.with_suffix('.intent.json')
    publication = release_tool.read_json(publication_path)
    require(publication.get('tag') == metadata['tag'] and publication.get('commit') == metadata['commit'], 'recovery publication identity belongs to another source')
    ready = release_tool.read_json(arguments.recovery_ready / 'recovery-ready.json')
    require(ready.get('state') == 'passed' and ready.get('commit') == metadata['commit'] and ready.get('tag') == metadata['tag'], 'recovery preparation belongs to another source')
    release_id = publication.get('release_id', publication.get('owned_release_id'))
    require(isinstance(release_id, int) and release_id > 0, 'recovery lacks durable owned release ID')
    files = asset_hashes(publication_files(arguments.candidate))
    require(publication.get('assets') == files, 'recovery publication assets differ from frozen candidate')
    candidate_record = github('releases/' + str(release_id))
    was_published = candidate_record.get('draft') is False
    validate_release(candidate_record, metadata['tag'], files, draft=not was_published, immutable=was_published)
    initial_latest = latest_identity()
    _prefix, initial_project = pages_project()
    initial_production = initial_project.get('canonical_deployment')
    require(initial_latest is None or initial_latest['id'] == release_id or initial_latest == baseline['previous_latest'], 'recovery conflict: latest belongs to another run')
    require((initial_production or {}).get('id') == (baseline['previous_production'] or {}).get('id') or initial_production and initial_production.get('deployment_trigger', {}).get('metadata', {}).get('commit_hash') == metadata['commit'] and initial_production.get('deployment_trigger', {}).get('metadata', {}).get('commit_message', '').startswith('ilium-run:' + os.environ['GITHUB_RUN_ID'] + ':'), 'recovery conflict: Pages belongs to another run')
    outcome = {'schema': 1, 'state': 'incomplete', 'tag': metadata['tag'], 'commit': metadata['commit'], 'latest': None, 'pages': None}
    errors = []
    def restore_latest():
        try:
            current = latest_identity()
            prior = baseline['previous_latest']
            require(current is None or current['id'] == release_id or current == prior, 'latest recovery refuses a channel owned by another run')
            if prior:
                previous = github('releases/' + str(prior['id']))
                require(previous.get('immutable') is True and previous.get('tag_name') == prior['tag'] and {item['name']: item.get('digest') for item in previous['assets']} == prior['assets'], 'previous immutable release bytes changed')
                if current != prior:
                    recovery_write(arguments, 'restore-latest', lambda: github('releases/' + str(prior['id']), method='PATCH', payload={'make_latest': 'true'}), latest_identity, lambda actual: actual == prior)
            if was_published and not candidate_record.get('prerelease'):
                recovery_write(arguments, 'quarantine-candidate', lambda: github('releases/' + str(release_id), method='PATCH', payload={'prerelease': True, 'make_latest': 'false'}), lambda: github('releases/' + str(release_id)), lambda actual: actual.get('id') == release_id and actual.get('prerelease') is True and actual.get('immutable') is True and {item['name']: item.get('digest') for item in actual.get('assets', [])} == {name: 'sha256:' + digest for name, digest in files.items()})
            assert_latest(baseline)
            validate_release(github('releases/' + str(release_id)), metadata['tag'], files, draft=not was_published, immutable=was_published)
            outcome['latest'] = {'state': 'restored', 'previous_latest': prior, 'candidate_quarantined': was_published}
        except (ValueError, OSError) as error:
            errors.append('latest: ' + str(error)); outcome['latest'] = {'state': 'incomplete'}

    def restore_pages():
        try:
            prefix, project = pages_project()
            current = project.get('canonical_deployment')
            prior = baseline['previous_production']
            if (current or {}).get('id') == (prior or {}).get('id'):
                verify_baseline_bytes(baseline)
                outcome['pages'] = {'state': 'unchanged', 'previous_production': prior}
            else:
                require(current and current.get('deployment_trigger', {}).get('metadata', {}).get('commit_hash') == metadata['commit'] and current.get('deployment_trigger', {}).get('metadata', {}).get('commit_message', '').startswith('ilium-run:' + os.environ['GITHUB_RUN_ID'] + ':'), 'Pages recovery refuses a channel owned by another source')
                if prior:
                    recovery_write(arguments, 'rollback-pages', lambda: cloudflare(prefix + '/deployments/' + prior['id'] + '/rollback', method='POST'), lambda: pages_project()[1].get('canonical_deployment'), lambda actual: actual is not None and actual.get('id') == prior['id'])
                    _prefix, restored = pages_project()
                    require(restored.get('canonical_deployment', {}).get('id') == prior['id'], 'Pages rollback did not restore saved production ID')
                    verify_baseline_bytes(baseline)
                    outcome['pages'] = {'state': 'restored', 'deployment_id': prior['id']}
                else:
                    require(ready.get('withdrawal'), 'first-release production exposure lacks prepared withdrawal')
                    directory = arguments.recovery_ready / 'withdrawal'
                    # Verify saved preview again before promoting the same reviewed
                    # withdrawal bytes. A preview is never a rollback target.
                    verify_withdrawal(directory, ready['withdrawal']['url'])
                    already_withdrawn = False
                    try:
                        verify_withdrawal(directory, 'https://' + pages.HOST)
                        already_withdrawn = True
                    except (ValueError, OSError):
                        pass
                    deployment = current if already_withdrawn else upload_pages(directory, baseline['production_branch'], metadata['commit'], arguments.log)
                    require(deployment.get('environment') == 'production', 'withdrawal deployment is not production')
                    verify_withdrawal(directory, 'https://' + pages.HOST)
                    _prefix, restored = pages_project()
                    require(restored.get('canonical_deployment', {}).get('id') == deployment['id'], 'withdrawal production ID differs from readback')
                    outcome['pages'] = {'state': 'withdrawn', 'deployment_id': deployment['id']}
        except (ValueError, OSError) as error:
            errors.append('pages: ' + str(error)); outcome['pages'] = {'state': 'incomplete'}

    outcome['fault'] = getattr(arguments, 'fault', 'unknown')
    outcome['order'] = ['pages', 'latest'] if outcome['fault'] == 'script-delivery' else ['latest', 'pages']
    write_json(arguments.output.with_suffix('.intent.json'), {'schema': 1, 'operation': 'recover', 'tag': metadata['tag'], 'commit': metadata['commit'], 'owned_release_id': release_id, 'baseline': baseline, 'initial_latest': initial_latest, 'initial_production': {key: (initial_production or {}).get(key) for key in ('id', 'environment', 'url', 'latest_stage', 'deployment_trigger')}, 'order': outcome['order']})
    operations = {'latest': restore_latest, 'pages': restore_pages}
    for operation in outcome['order']:
        operations[operation]()
        write_json(arguments.output.with_suffix('.' + operation + '.json'), outcome[operation])
    outcome['errors'] = errors
    outcome['state'] = 'incomplete' if errors else 'restored'
    write_json(arguments.output, outcome)
    emit('result' if not errors else 'error', command='recover', state=outcome['state'], output=str(arguments.output.resolve()))
    require(not errors, 'dual-channel recovery is incomplete; retain its journal and provider identities')


def parser():
    result = release_tool.JsonArgumentParser(description=__doc__, allow_abbrev=False)
    commands = result.add_subparsers(dest='command', required=True)
    for name in ('source', 'native', 'aggregate', 'qualify', 'draft', 'publish', 'latest', 'deploy', 'install', 'baseline', 'recovery-ready', 'readback', 'recover'):
        command = commands.add_parser(name, allow_abbrev=False)
        command.add_argument('--manifest', type=Path, default=ROOT / 'release/targets.toml')
        command.add_argument('--workspace', type=Path, default=ROOT / 'Cargo.toml')
        if name in {'source', 'native', 'aggregate'}:
            command.add_argument('--tag', required=name != 'source')
        if name == 'baseline':
            command.add_argument('--output', type=Path, required=True)
        if name in {'publish', 'latest', 'deploy', 'install', 'qualify', 'recovery-ready', 'recover'}:
            command.add_argument('--baseline', type=Path, required=name not in {'install', 'qualify'})
        if name in {'latest', 'deploy', 'readback', 'recover'}:
            command.add_argument('--publication-receipt', type=Path, required=True)
        if name in {'latest', 'recover'}:
            command.add_argument('--recovery-ready', type=Path, required=True)
        if name == 'readback':
            command.add_argument('--production-receipt', type=Path, required=True)
        if name in {'recovery-ready', 'recover'}:
            command.add_argument('--log', type=Path, required=True)
        if name == 'recover':
            command.add_argument('--fault', choices=('script-delivery', 'binary', 'unknown'), default='unknown')
        if name == 'native':
            command.add_argument('--target', required=True)
            command.add_argument('--runner-identity', required=True)
            command.add_argument('--work', type=Path, required=True)
            command.add_argument('--output', type=Path, required=True)
        if name == 'aggregate':
            command.add_argument('--artifacts', type=Path, required=True)
            command.add_argument('--output', type=Path, required=True)
        if name in {'qualify', 'draft', 'publish', 'latest', 'deploy', 'install', 'recovery-ready', 'readback', 'recover'}:
            command.add_argument('--candidate', type=Path, required=True)
            command.add_argument('--output', type=Path, required=True)
        if name in {'qualify', 'latest'}:
            command.add_argument('--receipts', type=Path, required=True)
            command.add_argument('--receipt-prefix', default='install-')
        if name == 'qualify':
            command.add_argument('--previous', action='store_true')
            command.add_argument('--preview-tag', action='store_true')
            command.add_argument('--preview-receipt', type=Path)
            command.add_argument('--public', action='store_true')
            command.add_argument('--literal', action='store_true')
        if name == 'publish':
            command.add_argument('--draft-receipt', type=Path, required=True)
        if name == 'deploy':
            command.add_argument('--production', action='store_true')
            command.add_argument('--log', type=Path, required=True)
        if name == 'install':
            command.add_argument('--native', type=Path, required=True)
            command.add_argument('--target', required=True)
            command.add_argument('--mode', choices=('candidate', 'github', 'preview-tag', 'previous', 'preview', 'public'), required=True)
            command.add_argument('--preview-receipt', type=Path)
            command.add_argument('--log', type=Path, required=True)
    return result


def main(argv=None):
    try:
        arguments = parser().parse_args(argv)
        handlers = {'source': source, 'native': native, 'aggregate': aggregate, 'qualify': qualify, 'draft': draft, 'publish': publish, 'latest': latest, 'deploy': deploy, 'install': install, 'baseline': capture_baseline, 'recovery-ready': recovery_ready, 'readback': readback, 'recover': recover}
        handlers[arguments.command](arguments)
        return 0
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError) as error:
        emit('error', message=str(error)[:1200])
        return 1


if __name__ == '__main__':
    sys.exit(main())
