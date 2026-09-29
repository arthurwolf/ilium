#!/usr/bin/env python3
"""Native clean-install gate using explicit assets and existing real PTY tests.

Local-asset qualification does not certify public HTTPS acquisition. All home,
config, runtime and installation state is task-owned; no user PATH is modified.
"""
from __future__ import annotations
import hashlib
import http.server
import json
import math
import os
from pathlib import Path
import re
import shutil
import ssl
import subprocess
import sys
import tempfile
import threading
import urllib.request
from urllib.parse import urlparse

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'release/scripts'))
from audit_native import native_identity, run
from release_tool import JsonArgumentParser, ReleaseError, emit, load_targets, selected_target

PUBLIC_ORIGIN = 'https://github.com/arthurwolf/ilium/releases'
PUBLIC_COMMANDS = {
    'posix': "curl --proto '=https' --tlsv1.2 -LsSf https://ilium-setup.pages.dev/install.sh | sh",
    'windows': 'irm https://ilium-setup.pages.dev/install.ps1 | iex',
}
TESTS = ('attaching_tui_renders_the_pane_created_by_new_pane_and_responds_to_the_help_keystroke',
         'right_click_restart_reloads_only_the_client_and_preserves_the_server')
EMBEDDING_FILES = ('model.onnx', 'tokenizer.json', 'config.json', 'special_tokens_map.json', 'tokenizer_config.json')


def require(condition, message):
    if not condition:
        raise ReleaseError(message)


def sha(path):
    result = hashlib.sha256()
    with Path(path).open('rb') as source:
        while block := source.read(65536):
            result.update(block)
    return result.hexdigest()


def evidence_inventory(directory):
    directory = Path(directory)
    require(directory.is_dir() and not directory.is_symlink(), 'Evidence directory must be plain')
    values = {}
    for path in sorted(directory.rglob('*')):
        require(not path.is_symlink() and (path.is_dir() or path.is_file()), 'Evidence contains a link or special entry')
        if path.is_file():
            values[path.relative_to(directory).as_posix()] = sha(path)
    require(values, 'Evidence inventory cannot be empty')
    return values


def evidence_digest(values):
    require(isinstance(values, dict) and values, 'Evidence inventory is missing')
    return hashlib.sha256(json.dumps(values, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def file_reference(path, root):
    path, root = Path(path), Path(root)
    require(path.is_file() and not path.is_symlink() and path.resolve().is_relative_to(root.resolve()),
            'Evidence reference must be a retained plain file')
    return {'path': path.relative_to(root).as_posix(), 'sha256': sha(path)}


def retain_json(artifacts, name, value):
    path = Path(artifacts) / name
    require(re.fullmatch(r'[A-Za-z0-9_.-]+\.json', name) and not path.exists(), 'Evidence JSON path must be new and safe')
    path.write_text(json.dumps(value, sort_keys=True, indent=2) + '\n', encoding='utf-8')
    return file_reference(path, artifacts)


def value_digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def validate_embedding_inputs(arguments):
    paths = (arguments.embedding_wrapper, arguments.embedding_command,
             arguments.embedding_model, arguments.embedding_model_register)
    require(all(path.is_absolute() and path.is_file() and not path.is_symlink() for path in paths),
            'Embedding inputs must be absolute plain files')
    expected_files = json.loads(arguments.expected_embedding_model_files)
    expected_runtime_files = json.loads(arguments.expected_embedding_runtime_files)
    require(isinstance(expected_files, dict) and set(expected_files) == set(EMBEDDING_FILES),
            'Expected embedding model inventory is invalid')
    require(isinstance(expected_runtime_files, dict) and all(
            re.fullmatch(r'[A-Za-z0-9._-]+', name) and re.fullmatch(r'[0-9a-f]{64}', digest)
            for name, digest in expected_runtime_files.items()), 'Expected embedding runtime inventory is invalid')
    require(sha(arguments.embedding_wrapper) == arguments.expected_embedding_wrapper_sha256,
            'Embedding wrapper differs from aggregate binding')
    require(sha(arguments.embedding_command) == arguments.expected_embedding_command_sha256,
            'Embedding command differs from aggregate binding')
    require(sha(arguments.embedding_model_register) == arguments.expected_embedding_model_register_sha256,
            'Embedding model register differs from aggregate binding')
    specification = json.loads(arguments.embedding_command.read_text(encoding='utf-8'))
    register = json.loads(arguments.embedding_model_register.read_text(encoding='utf-8'))
    require(specification.get('schema') == 1 and specification.get('state') == 'reviewed' and
            specification.get('protocol') == 'held-installed-process-v1' and
            specification.get('sha256') == arguments.expected_embedding_wrapper_sha256,
            'Embedding command is not bound to reviewed wrapper bytes')
    require(register.get('schema') == 1 and register.get('reviewed') is True and
            register.get('dimension') == 384 and register.get('files') == expected_files,
            'Embedding model register differs from aggregate model inventory')
    require(arguments.embedding_model.name == 'model.onnx' and
            all((arguments.embedding_model.parent / name).is_file() and
                not (arguments.embedding_model.parent / name).is_symlink() and
                sha(arguments.embedding_model.parent / name) == digest
                for name, digest in expected_files.items()),
            'Retained embedding model differs from aggregate binding')
    return {'wrapper_sha256': arguments.expected_embedding_wrapper_sha256,
            'command_sha256': arguments.expected_embedding_command_sha256,
            'model_register_sha256': arguments.expected_embedding_model_register_sha256,
            'model_files': expected_files, 'runtime_files': expected_runtime_files}


def validate_installed_embedding(proof, binding, installed_client_sha256, operating_system, installed_directory=None):
    vector = proof.get('embedding')
    require(proof.get('type') == 'embedding-proof' and proof.get('binary_sha256') == installed_client_sha256,
            'Embedding proof did not exercise the installed client bytes')
    require(proof.get('model_sha256') == binding['model_files']['model.onnx'],
            'Embedding proof used different model bytes')
    require(isinstance(vector, list) and len(vector) == 384 and
            all(type(value) in (int, float) and math.isfinite(value) for value in vector) and
            any(value != 0 for value in vector), 'Embedding proof is not a finite nonzero 384-vector')
    if operating_system in ('macos', 'windows'):
        require(bool(proof.get('loaded_runtime')), 'Embedding proof lacks shipped runtime identity')
    runtime_sha256 = None
    if proof.get('loaded_runtime'):
        runtime = Path(proof['loaded_runtime'])
        require(installed_directory is not None and runtime.is_absolute() and runtime.is_file() and
                not runtime.is_symlink() and runtime.parent.resolve() == Path(installed_directory).resolve(),
                'Embedding proof runtime is not the shipped installed runtime')
        runtime_sha256 = sha(runtime)
    return {**binding, 'state': 'passed', 'binary_sha256': installed_client_sha256,
            'model_sha256': proof['model_sha256'], 'dimension': 384, 'finite_nonzero': True,
            'loaded_runtime': proof.get('loaded_runtime', ''),
            'runtime_sha256': runtime_sha256,
            'vector_sha256': hashlib.sha256(json.dumps(vector, allow_nan=False).encode()).hexdigest()}


def check_assets(arguments, target):
    directory = arguments.archive_directory
    require(directory.is_absolute() and directory.is_dir() and not directory.is_symlink(), 'Explicit plain archive directory required')
    sums = directory / 'SHA256SUMS'
    require(sums.is_file() and not sums.is_symlink() and sums.stat().st_size <= 16384, 'Five-target checksums missing')
    expected = {item['archive'] for item in load_targets(arguments.manifest)}
    records = sums.read_text(encoding='utf-8').splitlines()
    parsed = {}
    for line in records:
        match = re.fullmatch(r'([0-9a-f]{64})  ([A-Za-z0-9_.-]+)', line)
        require(match and match[2] not in parsed, 'Checksum inventory malformed or duplicate')
        parsed[match[2]] = match[1]
    require(set(parsed) == expected, 'Installer acceptance requires exact five-target checksum inventory')
    archive = directory / target['archive']
    require(archive.is_file() and not archive.is_symlink() and sha(archive) == parsed[archive.name], 'Native archive differs from aggregated checksum')
    return {'archive_sha256': parsed[archive.name], 'checksums_sha256': sha(sums)}


class LocalReleases(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        expected = '/releases/download/' + self.server.tag + '/'
        name = self.path.removeprefix(expected)
        if self.path.startswith(expected) and name in self.server.members:
            path = self.server.assets / name
            data = path.read_bytes()
            self.server.requests.append({'path': self.path, 'sha256': hashlib.sha256(data).hexdigest()})
            self.send_response(200)
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        else:
            self.send_error(404)

    def log_message(self, *arguments):
        pass


def serve_assets(arguments, target, temporary):
    certificate, key = temporary / 'certificate.pem', temporary / 'key.pem'
    run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', key,
         '-out', certificate, '-days', '1', '-subj', '/CN=127.0.0.1', '-addext', 'subjectAltName=IP:127.0.0.1'])
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), LocalReleases)
    server.assets, server.tag = arguments.archive_directory, arguments.tag
    server.members = {target['archive'], 'SHA256SUMS'}
    server.requests = []
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(certificate, key)
    server.socket = context.wrap_socket(server.socket, server_side=True)
    thread = threading.Thread(target=server.serve_forever, daemon=True); thread.start()
    return server, 'https://127.0.0.1:' + str(server.server_port) + '/releases', certificate


def isolated_environment(root):
    env = dict(os.environ)
    paths = {name: root / name.lower() for name in ('HOME', 'USERPROFILE', 'LOCALAPPDATA', 'APPDATA', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_RUNTIME_DIR')}
    for name, path in paths.items():
        path.mkdir()
        env[name] = str(path)
    env['XDG_BIN_HOME'] = str(paths['HOME'] / '.local/bin')
    env.update(TERM='xterm-256color', COLORTERM='truecolor', ILIUM_CONFIG_DIR=str(root / 'ilium-config'), ILIUM_AGENT_SETUP_HOME=str(root / 'agent-home'), ILIUM_DEBUG_LOG_DIR=str(root / 'debug'), HF_HUB_OFFLINE='1')
    # Avoid macOS XDG socket-length assumptions; the Rust PTY fixture creates
    # its own short runtime endpoint and platform-specific config overrides.
    return env


def windows_local_script(installer, output):
    source = installer.read_text(encoding='utf-8')
    footer = 'Invoke-IliumInstall -Version $Version -InstallDir $InstallDir -BinDir $BinDir -NoModifyPath:$NoModifyPath -Uninstall:$Uninstall'
    require(source.rstrip().endswith(footer), 'Windows installer entrypoint changed; local harness cannot safely remove it')
    # Preserve the complete original function bodies. Only the reviewed download
    # adapter is replaced for local bytes; the exact production installer hash
    # and extracted-source hash are retained separately in the final evidence.
    body = source.rstrip()[:-len(footer)]
    override = r'''
function Save-IliumDownload([string]$Url, [string]$Destination) {
 $uri=[Uri]$Url
 if ($uri.Scheme -cne 'https' -or $uri.Host -cne 'github.com' -or $uri.UserInfo -or $uri.Port -ne 443) { throw 'Unexpected local-fixture URL' }
 $prefix='/arthurwolf/ilium/releases/download/'+$env:ILIUM_NATIVE_TAG+'/'
 if (-not $uri.AbsolutePath.StartsWith($prefix,[StringComparison]::Ordinal)) { throw 'Unexpected local-fixture tag' }
 $name=$uri.AbsolutePath.Substring($prefix.Length)
 if ($name -notin @('ilium-windows-x86_64.zip','SHA256SUMS')) { throw 'Unexpected local-fixture member' }
 [IO.File]::Copy((Join-Path $env:ILIUM_NATIVE_ASSETS $name),$Destination,$false)
}
if ($env:ILIUM_NATIVE_UNINSTALL -ceq 'true') {
 Invoke-IliumInstall -Version $env:ILIUM_NATIVE_VERSION -InstallDir $env:ILIUM_NATIVE_INSTALL -BinDir $env:ILIUM_NATIVE_BIN -Uninstall
} else {
 Invoke-IliumInstall -Version $env:ILIUM_NATIVE_VERSION -InstallDir $env:ILIUM_NATIVE_INSTALL -BinDir $env:ILIUM_NATIVE_BIN
}
'''
    output.write_text(body + override, encoding='utf-8')
    return sha(output)


def verify_pty_result(result, name):
    require(result.returncode == 0 and re.search(r'test result: ok\. 1 passed; 0 failed;', result.stdout), 'Real installed PTY acceptance failed or selected zero tests: ' + name + ': ' + result.stdout[-2000:] + result.stderr[-2000:])


def public_script_hash(operating_system, script_url=None):
    url = script_url or 'https://ilium-setup.pages.dev/install.' + ('ps1' if operating_system == 'windows' else 'sh')
    # Independent readback binds the static public deployment around literal
    # execution; it never replaces the URL or bytes consumed by that command.
    with urllib.request.urlopen(url, timeout=60) as response:
        require(response.geturl() == url and response.status == 200, 'Public installer URL redirected or failed')
        content = response.read(1_000_001)
    require(0 < len(content) <= 1_000_000, 'Public script is empty or oversized')
    return hashlib.sha256(content).hexdigest()


def preview_command(script_url, operating_system):
    parsed = urlparse(script_url)
    extension = 'ps1' if operating_system == 'windows' else 'sh'
    require(parsed.scheme == 'https' and parsed.username is None and parsed.password is None and parsed.port is None and not parsed.query and not parsed.fragment and re.fullmatch(r'[a-z0-9-]+\.ilium-setup\.pages\.dev', parsed.hostname or '') and parsed.path == '/install.' + extension, 'Preview script must be the exact HTTPS installer URL of an ilium-setup Pages preview deployment')
    return PUBLIC_COMMANDS['windows' if operating_system == 'windows' else 'posix'].replace('https://ilium-setup.pages.dev/install.' + extension, script_url)


def windows_disposable_identity(environment):
    require(environment.get('GITHUB_ACTIONS') == 'true' and environment.get('RUNNER_ENVIRONMENT') == 'github-hosted', 'Literal Windows install requires the authorized disposable GitHub-hosted account; never use a user workstation')
    powershell = shutil.which('powershell.exe')
    require(powershell, 'Native Windows PowerShell required')
    command = "@{local_app_data=[Environment]::GetFolderPath('LocalApplicationData'); user_profile=[Environment]::GetFolderPath('UserProfile'); user_path=[Environment]::GetEnvironmentVariable('Path','User'); user=[Environment]::UserName} | ConvertTo-Json -Compress"
    identity = json.loads(run([powershell, '-NoProfile', '-NonInteractive', '-Command', command], environment).stdout)
    require(identity.get('local_app_data') and identity.get('user_profile') and identity.get('user'), 'Disposable native Windows account identity unavailable')
    return identity


def restore_windows_path(environment, value):
    powershell = shutil.which('powershell.exe')
    require(powershell, 'Native Windows PowerShell required for PATH restoration')
    environment = dict(environment)
    environment['ILIUM_NATIVE_RESTORE_PATH'] = '' if value is None else value
    environment['ILIUM_NATIVE_RESTORE_PATH_WAS_NULL'] = 'true' if value is None else 'false'
    script = "$value=$env:ILIUM_NATIVE_RESTORE_PATH; if($env:ILIUM_NATIVE_RESTORE_PATH_WAS_NULL -ceq 'true'){$value=$null}; [Environment]::SetEnvironmentVariable('Path',$value,'User'); if([Environment]::GetEnvironmentVariable('Path','User') -cne $value){throw 'PATH restore readback failed'}"
    run([powershell, '-NoProfile', '-NonInteractive', '-Command', script], environment)


def finalize_windows_cleanup(environment, original_path, ownership_cleanup):
    """Run owned cleanup and independently restore exact disposable PATH bytes."""
    failures = []
    try:
        ownership_cleanup()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        failures.append('ownership cleanup: ' + str(error))
    try:
        current = windows_disposable_identity(environment)
        if current.get('user_path') != original_path:
            restore_windows_path(environment, original_path)
        restored = windows_disposable_identity(environment)
        require(restored.get('user_path') == original_path, 'exact user PATH restoration readback failed')
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        failures.append('PATH restoration: ' + str(error))
    require(not failures, '; '.join(failures))


def installed_pair(install, version):
    return Path(install) / 'versions' / version / 'bin'


def tree_inventory(root):
    root = Path(root)
    require(root.is_dir() and not root.is_symlink(), 'Owned install state is not a plain directory')
    values = {}
    for path in sorted(root.rglob('*')):
        require(not path.is_symlink() and (path.is_dir() or path.is_file()), 'Owned install state contains a link or special entry')
        if path.is_file():
            values[path.relative_to(root).as_posix()] = sha(path)
    return values


def relabel_prior_fixture(install, target, version):
    """Relabel one audited install as an older owned transaction fixture.

    Executable bytes remain exact candidate bytes. The synthetic version label is
    confined to task-owned installer metadata and is never release evidence.
    """
    prior = '0.0.0-task7b-prior'
    versions = install / 'versions'
    current = versions / version
    destination = versions / prior
    require(current.is_dir() and not destination.exists(), 'Current fixture version is unavailable for prior relabel')
    current.rename(destination)
    state = install / 'installer-state'
    if target['os'] == 'windows':
        receipt = state / ('version-' + version + '.json')
        prior_receipt = state / ('version-' + prior + '.json')
    else:
        receipt = state / ('version-' + version)
        prior_receipt = state / ('version-' + prior)
    require(receipt.is_file() and not receipt.is_symlink(), 'Owned fixture receipt is missing')
    receipt.rename(prior_receipt)
    (install / 'current').write_text(prior + '\n', encoding='ascii')
    return prior


def run_installer(command, environment, artifacts, label, *, succeeds):
    result = subprocess.run(command, env=environment, capture_output=True, text=True, timeout=900)
    stdout = artifacts / (label + '-stdout.txt')
    stderr = artifacts / (label + '-stderr.txt')
    stdout.write_text(result.stdout, encoding='utf-8')
    stderr.write_text(result.stderr, encoding='utf-8')
    require((result.returncode == 0) == succeeds,
            label + ' installer result differed: ' + result.stdout[-2000:] + result.stderr[-2000:])
    return {'exit_code': result.returncode, 'stdout': file_reference(stdout, artifacts),
            'stderr': file_reference(stderr, artifacts)}


def lifecycle_snapshot(install, bin_directory, path_state, version):
    pointer = install / 'current'
    return {'schema': 1, 'version': version,
            'pointer': pointer.read_text(encoding='ascii') if pointer.is_file() and not pointer.is_symlink() else None,
            'install_files': tree_inventory(install), 'launcher_files': tree_inventory(bin_directory),
            'path_state_sha256': value_digest(path_state)}


def run_installed_embedding(arguments, target, pair, binding, artifacts, environment):
    if target['os'] == 'macos':
        specification = json.loads(arguments.embedding_command.read_text(encoding='utf-8'))
        specification['command'] = [str(arguments.embedding_wrapper), '--model-lock', str(arguments.embedding_model_register)]
        command_path = artifacts / 'installed-embedding-command.json'
        command_path.write_text(json.dumps(specification, sort_keys=True, indent=2) + '\n', encoding='utf-8')
        from audit_native import embedding_gate
        receipt, native_evidence = embedding_gate(command_path, pair, arguments.embedding_model)
        for name in ('stdout', 'stderr', 'observed_process', 'native_mappings'):
            (artifacts / ('embedding-' + name + '.txt')).write_text(native_evidence[name], encoding='utf-8')
        records = [json.loads(line) for line in native_evidence['stdout'].splitlines()]
        proof = [record for record in records if record.get('type') == 'embedding-proof']
        require(len(proof) == 1, 'macOS embedding evidence lacks its unique raw proof')
        return {**binding, 'state': 'passed', 'binary_sha256': sha(pair / 'ilium'),
                'model_sha256': receipt['model_sha256'], 'dimension': receipt['dimensions'],
                'finite_nonzero': True, 'loaded_runtime': receipt['loaded_runtime'],
                'runtime_sha256': receipt['runtime_sha256'], 'vector_sha256': receipt['vector_sha256'],
                'executable_path': proof[0]['executable_path'], 'process_id': proof[0]['ilium_pid'],
                'stdout': file_reference(artifacts / 'embedding-stdout.txt', artifacts),
                'stderr': file_reference(artifacts / 'embedding-stderr.txt', artifacts),
                'observed_process': file_reference(artifacts / 'embedding-observed_process.txt', artifacts),
                'native_mappings': file_reference(artifacts / 'embedding-native_mappings.txt', artifacts),
                'native_mapping_verified': True, 'observed_with': ['/bin/ps', '/usr/bin/vmmap']}
    invocation = [sys.executable, str(arguments.embedding_wrapper), '--installed-directory', str(pair),
                  '--model', str(arguments.embedding_model), '--text', 'release embedding acceptance',
                  '--model-lock', str(arguments.embedding_model_register)]
    result = subprocess.run(invocation, env=environment, capture_output=True, text=True, timeout=600)
    (artifacts / 'embedding-stdout.txt').write_text(result.stdout, encoding='utf-8')
    (artifacts / 'embedding-stderr.txt').write_text(result.stderr, encoding='utf-8')
    records = [json.loads(line) for line in result.stdout.splitlines()]
    require(result.returncode == 0 and len(records) == 1, 'Installed embedding harness failed or emitted ambiguous output')
    receipt = validate_installed_embedding(records[0], binding, sha(pair / target['executables'][0]), target['os'], pair)
    return {**receipt, 'executable_path': records[0]['executable_path'], 'process_id': records[0]['ilium_pid'],
            'stdout': file_reference(artifacts / 'embedding-stdout.txt', artifacts),
            'stderr': file_reference(artifacts / 'embedding-stderr.txt', artifacts)}


def qualify(arguments):
    target = selected_target(arguments.manifest, arguments.target)
    identity = native_identity(target, arguments.runner_identity)
    require(re.fullmatch(r'v[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?', arguments.tag), 'Explicit safe release tag required')
    version = arguments.tag[1:]
    for value in (arguments.expected_client_sha256, arguments.expected_server_sha256):
        require(re.fullmatch('[0-9a-f]{64}', value), 'Explicit paired binary hashes required')
    literal = arguments.literal_public_command
    preview = arguments.pages_script_url
    require(not (literal and preview), 'Preview and literal production gates are distinct')
    public_command = literal or (preview_command(preview, target['os']) if preview else None)
    require(not literal or literal == PUBLIC_COMMANDS['windows' if target['os'] == 'windows' else 'posix'], 'Literal public command differs from the exact canonical published command')
    require(not public_command or arguments.origin == PUBLIC_ORIGIN, 'Literal no-argument command requires final public origin mode')
    require(arguments.origin in ('local', PUBLIC_ORIGIN), 'Origin must be local native qualification or exact public release origin')
    require(arguments.installer.is_absolute() and arguments.installer.is_file() and not arguments.installer.is_symlink(), 'Explicit plain installer required')
    require(arguments.native_test_binary.is_absolute() and arguments.native_test_binary.is_file() and not arguments.native_test_binary.is_symlink(), 'Prebuilt native pty_tui_smoke executable required')
    require(arguments.output.is_absolute() and not arguments.output.exists(), 'Receipt output must be a new absolute path')
    assets = check_assets(arguments, target)
    embedding_binding = validate_embedding_inputs(arguments)
    artifacts = arguments.output.with_suffix('.evidence')
    require(not artifacts.exists(), 'Evidence output directory already exists')
    artifacts.mkdir()
    with tempfile.TemporaryDirectory(prefix='ilium-native-install-') as temporary_name:
        temporary = Path(temporary_name)
        env = isolated_environment(temporary)
        if arguments.origin == PUBLIC_ORIGIN:
            env.pop('ILIUM_INSTALL_TEST_ORIGIN', None)
        env['SHELL'] = '/bin/sh'
        install, bin_directory = temporary / 'install', temporary / 'bin'
        if not public_command:
            env.update(ILIUM_NATIVE_ASSETS=str(arguments.archive_directory), ILIUM_NATIVE_TAG=arguments.tag, ILIUM_NATIVE_VERSION=version, ILIUM_NATIVE_INSTALL=str(install), ILIUM_NATIVE_BIN=str(bin_directory))
        else:
            for variable in ('ILIUM_NATIVE_ASSETS', 'ILIUM_NATIVE_TAG', 'ILIUM_NATIVE_VERSION', 'ILIUM_NATIVE_INSTALL', 'ILIUM_NATIVE_BIN'):
                env.pop(variable, None)
        server = None
        windows_account_before = windows_disposable_identity(env) if target['os'] == 'windows' else None
        windows_account_after = None
        launcher_results = []
        scenarios = {}
        owned_uninstall_completed = False
        public_before = public_script_hash(target['os'], preview) if public_command else None
        require(not public_command or public_before == sha(arguments.installer), 'Published installer bytes differ from qualified canonical source')
        source = arguments.installer
        source_mode = 'unmodified-installer'
        try:
            if public_command:
                source_mode = 'preview-no-argument-public-command' if preview else 'literal-no-argument-public-command'
                if target['os'] == 'windows':
                    install = Path(windows_account_before['local_app_data']) / 'ilium'
                    bin_directory = install / 'bin'
                    require(not install.exists(), 'Disposable account already contains an installation; clean literal acceptance cannot overwrite it')
                    command = [shutil.which('powershell.exe'), '-NoProfile', '-NonInteractive', '-Command', public_command]
                else:
                    install = Path(env['XDG_DATA_HOME']) / 'ilium'
                    bin_directory = Path(env['HOME']) / '.local/bin'
                    command = ['/bin/sh', '-c', public_command]
            elif target['os'] == 'windows':
                powershell = shutil.which('powershell.exe')
                require(powershell, 'Native Windows PowerShell required')
                if arguments.origin == 'local':
                    source = temporary / 'local-install.ps1'
                    source_mode = 'exact-functions-with-local-download-adapter'
                    windows_local_script(arguments.installer, source)
                    shutil.copy2(source, artifacts / 'local-install.ps1')
                    command = [powershell, '-NoProfile', '-NonInteractive', '-File', str(source)]
                else:
                    command = [powershell, '-NoProfile', '-NonInteractive', '-File', str(source), '-Version', version, '-InstallDir', str(install), '-BinDir', str(bin_directory)]
            else:
                if arguments.origin == 'local':
                    server, origin, certificate = serve_assets(arguments, target, temporary)
                    env.update(ILIUM_INSTALL_TEST_ORIGIN=origin, CURL_CA_BUNDLE=str(certificate), SSL_CERT_FILE=str(certificate))
                else:
                    env.pop('ILIUM_INSTALL_TEST_ORIGIN', None)
                command = ['/bin/sh', str(arguments.installer), '--version', version, '--install-dir', str(install), '--bin-dir', str(bin_directory)]
            profile = Path(env['HOME']) / '.profile'
            if target['os'] != 'windows':
                profile.write_text('# task-owned authored profile sentinel\n', encoding='utf-8')
            original_path_state = windows_account_before.get('user_path') if target['os'] == 'windows' else profile.read_text(encoding='utf-8')
            initial = run_installer(command, env, artifacts, 'initial-install', succeeds=True)
            if arguments.origin == 'local':
                prior = relabel_prior_fixture(install, target, version)
                prior_state = tree_inventory(install)
                prior_launchers = tree_inventory(bin_directory)
                prior_path_state = (windows_disposable_identity(env).get('user_path') if target['os'] == 'windows'
                                    else profile.read_text(encoding='utf-8'))
                prior_snapshot_value = lifecycle_snapshot(install, bin_directory, prior_path_state, prior)
                prior_snapshot = retain_json(artifacts, 'prior-state.json', prior_snapshot_value)
                corrupt_assets = temporary / 'corrupt-assets'
                shutil.copytree(arguments.archive_directory, corrupt_assets)
                with (corrupt_assets / target['archive']).open('ab') as corrupt:
                    corrupt.write(b'\ncorrupt-task7b-candidate\n')
                if target['os'] == 'windows':
                    env['ILIUM_NATIVE_ASSETS'] = str(corrupt_assets)
                else:
                    server.assets = corrupt_assets
                failed = run_installer(command, env, artifacts, 'corrupt-candidate', succeeds=False)
                require(tree_inventory(install) == prior_state and tree_inventory(bin_directory) == prior_launchers,
                        'Corrupt candidate changed prior pointer, pair, launchers or ownership state')
                current_path_state = (windows_disposable_identity(env).get('user_path') if target['os'] == 'windows'
                                      else profile.read_text(encoding='utf-8'))
                require(current_path_state == prior_path_state,
                        'Corrupt candidate changed PATH or task-owned profile state')
                corrupt_after_value = lifecycle_snapshot(install, bin_directory, current_path_state, prior)
                require(corrupt_after_value == prior_snapshot_value, 'Corrupt candidate snapshot differs from prior fixture')
                corrupt_after = retain_json(artifacts, 'corrupt-after-state.json', corrupt_after_value)
                scenarios['prior_fixture'] = {'state': 'passed', 'kind': 'task-owned-audited-bytes', 'fixture_version': prior, 'candidate_version': version, 'installer': initial, 'snapshot': prior_snapshot}
                scenarios['corrupt_candidate_rollback'] = {'state': 'passed', 'installer': failed, 'before': prior_snapshot, 'after': corrupt_after}
                if target['os'] == 'windows':
                    env['ILIUM_NATIVE_ASSETS'] = str(arguments.archive_directory)
                else:
                    server.assets = arguments.archive_directory
                scenarios['upgrade'] = {'state': 'passed', 'installer': run_installer(command, env, artifacts, 'candidate-upgrade', succeeds=True), 'from': prior, 'to': version}
            else:
                scenarios['upgrade'] = {'state': 'passed', 'scope': 'initial-clean-public-or-previous', 'installer': initial, 'to': version}
            repeat_path_before = (windows_disposable_identity(env).get('user_path') if target['os'] == 'windows' else profile.read_text(encoding='utf-8'))
            before_repeat_value = lifecycle_snapshot(install, bin_directory, repeat_path_before, version)
            before_repeat = retain_json(artifacts, 'repeat-before-state.json', before_repeat_value)
            repeated = run_installer(command, env, artifacts, 'repeat-install', succeeds=True)
            repeat_path_after = (windows_disposable_identity(env).get('user_path') if target['os'] == 'windows' else profile.read_text(encoding='utf-8'))
            after_repeat_value = lifecycle_snapshot(install, bin_directory, repeat_path_after, version)
            require(after_repeat_value == before_repeat_value, 'Repeat install changed immutable owned installation state')
            after_repeat = retain_json(artifacts, 'repeat-after-state.json', after_repeat_value)
            scenarios['repeat'] = {'state': 'passed', 'installer': repeated, 'before': before_repeat, 'after': after_repeat, 'version': version}
            pair = installed_pair(install, version)
            expected = dict(zip(target['executables'], (arguments.expected_client_sha256, arguments.expected_server_sha256)))
            for name, value in expected.items():
                path = pair / name
                require(path.is_file() and not path.is_symlink() and sha(path) == value, 'Installed pair differs from audited release: ' + name)
            if public_command:
                public_after = public_script_hash(target['os'], preview)
                require(public_after == public_before, 'Public installer changed across literal command execution')
                if target['os'] == 'windows':
                    windows_account_after = windows_disposable_identity(env)
                    require(str(bin_directory).casefold() in [entry.strip().rstrip('\\/').casefold() for entry in (windows_account_after.get('user_path') or '').split(';')], 'Literal install did not persist the disposable account user PATH')
                for name in target['executables']:
                    launcher = bin_directory / (name.removesuffix('.exe') + '.cmd' if target['os'] == 'windows' else name)
                    if target['os'] == 'windows':
                        env['ILIUM_NATIVE_LAUNCHER'] = str(launcher)
                        invocation = [shutil.which('powershell.exe'), '-NoProfile', '-NonInteractive', '-Command', '& $env:ILIUM_NATIVE_LAUNCHER --version; exit $LASTEXITCODE']
                    else:
                        invocation = [str(launcher), '--version']
                    result = subprocess.run(invocation, env=env, capture_output=True, text=True, timeout=120)
                    require(result.returncode == 0 and result.stdout.strip() == name.removesuffix('.exe') + ' ' + version, 'Installed public launcher failed: ' + name)
                    launcher_results.append({'name': name, 'exit_code': result.returncode, 'stdout': result.stdout})
            if target['os'] == 'windows':
                windows_account_after = windows_disposable_identity(env)
                path_entries = [entry.strip().rstrip('\\/').casefold() for entry in (windows_account_after.get('user_path') or '').split(';')]
                require(path_entries.count(str(bin_directory).rstrip('\\/').casefold()) == 1, 'Windows installer PATH entry was not deduplicated')
                installed_path_state = windows_account_after.get('user_path')
                path_kind = 'windows-user-registry'
            else:
                profile_text = profile.read_text(encoding='utf-8')
                require(profile_text.count(str(bin_directory)) == 1, 'POSIX installer profile entry was not deduplicated')
                installed_path_state = profile_text
                path_kind = 'task-owned-posix-profile'
            installed_embedding = run_installed_embedding(arguments, target, pair, embedding_binding, artifacts, env)
            env['ILIUM_PTY_SMOKE_BINARY'] = str(pair / target['executables'][0])
            tests = []
            for name in TESTS:
                invocation = [str(arguments.native_test_binary), name, '--exact', '--nocapture', '--test-threads=1']
                result = subprocess.run(invocation, env=env, capture_output=True, text=True, timeout=900)
                (artifacts / (name + '.stdout.txt')).write_text(result.stdout)
                (artifacts / (name + '.stderr.txt')).write_text(result.stderr)
                verify_pty_result(result, name)
                tests.append({'name': name, 'command': list(map(str, invocation)), 'exit_code': result.returncode,
                              'stdout': file_reference(artifacts / (name + '.stdout.txt'), artifacts),
                              'stderr': file_reference(artifacts / (name + '.stderr.txt'), artifacts)})
            require(all(sha(pair / name) == value for name, value in expected.items()), 'Installed pair changed during native tests')
            scenarios['pty'] = {'state': 'passed', 'tests': tests}
            sentinel = bin_directory / 'task-owned-unrelated-sentinel.txt'
            sentinel.write_text('must survive ownership-aware uninstall\n', encoding='utf-8')
            if target['os'] == 'windows':
                if arguments.origin == 'local':
                    env['ILIUM_NATIVE_UNINSTALL'] = 'true'
                    uninstall_command = command
                else:
                    uninstall_command = [shutil.which('powershell.exe'), '-NoProfile', '-NonInteractive', '-File', str(arguments.installer), '-Uninstall']
            else:
                uninstall_command = ['/bin/sh', str(arguments.installer), '--uninstall', '--install-dir', str(install), '--bin-dir', str(bin_directory)]
            uninstall = run_installer(uninstall_command, env, artifacts, 'owned-uninstall', succeeds=True)
            env.pop('ILIUM_NATIVE_UNINSTALL', None)
            require(sentinel.is_file() and sentinel.read_text(encoding='utf-8') == 'must survive ownership-aware uninstall\n', 'Ownership-aware uninstall removed unrelated sentinel')
            require(not pair.exists() and all(not (bin_directory / (name.removesuffix('.exe') + '.cmd' if target['os'] == 'windows' else name)).exists() for name in target['executables']), 'Owned uninstall retained owned pair or launchers')
            if target['os'] == 'windows':
                restored = windows_disposable_identity(env)
                require(restored.get('user_path') == windows_account_before.get('user_path'), 'Disposable user PATH was not restored by owned uninstall')
                final_path_state = restored.get('user_path')
            else:
                require(profile.read_text(encoding='utf-8') == '# task-owned authored profile sentinel\n', 'POSIX owned profile block was not removed exactly')
                final_path_state = profile.read_text(encoding='utf-8')
            require(final_path_state == original_path_state, 'Uninstall did not restore exact PATH/profile bytes')
            path_record = {'schema': 1, 'kind': path_kind, 'bin_directory': str(bin_directory),
                           'entry_count': 1, 'original_sha256': value_digest(original_path_state),
                           'installed_sha256': value_digest(installed_path_state),
                           'repeat_sha256': value_digest(repeat_path_after),
                           'restored_sha256': value_digest(final_path_state),
                           'exact_values': ({'original': original_path_state, 'installed': installed_path_state,
                                             'repeat': repeat_path_after, 'restored': final_path_state}
                                            if target['os'] != 'windows' else None)}
            path_reference = retain_json(artifacts, 'path-lifecycle.json', path_record)
            scenarios['path_deduplication'] = {'state': 'passed', 'evidence': path_reference}
            uninstall_record = {'schema': 1, 'pair_removed': not pair.exists(),
                                'launchers_removed': all(not (bin_directory / (name.removesuffix('.exe') + '.cmd' if target['os'] == 'windows' else name)).exists() for name in target['executables']),
                                'path_restored': final_path_state == original_path_state,
                                'sentinel_name': sentinel.name, 'sentinel_sha256': sha(sentinel),
                                'sentinel_survived': sentinel.is_file()}
            scenarios['uninstall'] = {'state': 'passed', 'installer': uninstall,
                                      'evidence': retain_json(artifacts, 'uninstall-state.json', uninstall_record)}
            owned_uninstall_completed = True
            if arguments.origin != 'local':
                scenarios = {name: value for name, value in scenarios.items() if name in {'upgrade', 'repeat', 'path_deduplication', 'pty', 'uninstall'}}
            retained_evidence = evidence_inventory(artifacts)
            receipt = {'schema': 2, 'state': 'passed', 'publication_allowed': True, 'target': arguments.target, 'tag': arguments.tag, 'native_identity': identity, 'installer_sha256': sha(arguments.installer), 'installer_command': list(map(str, command)), 'literal_public_command': literal, 'pages_script_url': preview, 'preview_public_command': public_command if preview else None, 'public_script_sha256': public_before, 'windows_account_before': windows_account_before, 'windows_account_after': windows_account_after, 'launcher_results': launcher_results, 'installer_source_mode': source_mode, 'executed_installer_sha256': sha(source) if target['os'] == 'windows' else sha(arguments.installer), 'origin': arguments.origin, 'public_transport_verified': arguments.origin == PUBLIC_ORIGIN, 'archive': assets, 'archive_sha256': assets['archive_sha256'], 'installed_pair_directory': str(pair), 'installed_client_sha256': arguments.expected_client_sha256, 'installed_server_sha256': arguments.expected_server_sha256, 'installed_pair_sha256': expected, 'installed_embedding': installed_embedding, 'native_test_binary_sha256': sha(arguments.native_test_binary), 'pty_tests': tests, 'scenarios': scenarios, 'requests': server.requests if server else [], 'evidence_directory': str(artifacts), 'evidence_files': retained_evidence, 'evidence_files_sha256': evidence_digest(retained_evidence), 'isolated_state_cleaned': True}
        finally:
            if server:
                server.shutdown(); server.server_close()
            primary_error = sys.exc_info()[1]
            def cleanup_owned_installation():
                if owned_uninstall_completed or not (install / 'installer-state').is_dir():
                    return
                if target['os'] == 'windows':
                    powershell = shutil.which('powershell.exe')
                    if arguments.origin == 'local' and source != arguments.installer:
                        env['ILIUM_NATIVE_UNINSTALL'] = 'true'
                        cleanup_command = [powershell, '-NoProfile', '-NonInteractive', '-File', str(source)]
                    else:
                        cleanup_command = [powershell, '-NoProfile', '-NonInteractive', '-File', str(arguments.installer), '-Version', version, '-InstallDir', str(install), '-BinDir', str(bin_directory), '-Uninstall']
                else:
                    cleanup_command = ['/bin/sh', str(arguments.installer), '--uninstall', '--install-dir', str(install), '--bin-dir', str(bin_directory)]
                cleanup = subprocess.run(cleanup_command, env=env, capture_output=True, text=True, timeout=300)
                (artifacts / 'failure-cleanup-stdout.txt').write_text(cleanup.stdout, encoding='utf-8')
                (artifacts / 'failure-cleanup-stderr.txt').write_text(cleanup.stderr, encoding='utf-8')
                require(cleanup.returncode == 0, 'Ownership-aware failure cleanup did not complete')
            if target['os'] == 'windows' and windows_account_before is not None:
                try:
                    finalize_windows_cleanup(env, windows_account_before.get('user_path'), cleanup_owned_installation)
                except (OSError, ValueError, subprocess.SubprocessError) as cleanup_error:
                    if primary_error is not None:
                        raise ReleaseError('primary failure: ' + str(primary_error) + '; final cleanup failure: ' + str(cleanup_error)) from cleanup_error
                    raise
            else:
                cleanup_owned_installation()
    with arguments.output.open('x', encoding='utf-8') as output:
        json.dump(receipt, output, indent=2, sort_keys=True); output.write('\n')
    emit({'type': 'artifact', 'path': str(arguments.output), 'sha256': sha(arguments.output)})
    emit({'type': 'result', 'command': 'native-install', 'state': 'passed', 'publication_allowed': True, 'public_transport_verified': receipt['public_transport_verified'], 'target': arguments.target})


def parser():
    result = JsonArgumentParser(description=__doc__, allow_abbrev=False)
    for name in ('manifest', 'installer', 'archive-directory', 'native-test-binary', 'output', 'embedding-wrapper', 'embedding-command', 'embedding-model', 'embedding-model-register'):
        result.add_argument('--' + name, type=Path, required=True)
    for name in ('tag', 'target', 'expected-client-sha256', 'expected-server-sha256', 'expected-embedding-wrapper-sha256', 'expected-embedding-command-sha256', 'expected-embedding-model-register-sha256', 'expected-embedding-model-files', 'expected-embedding-runtime-files'):
        result.add_argument('--' + name, required=True)
    result.add_argument('--origin', default='local')
    result.add_argument('--literal-public-command', help='exact canonical no-argument public command; Windows requires a disposable hosted CI account')
    result.add_argument('--pages-script-url', help='exact verified ilium-setup Pages preview HTTPS installer URL; no version switches')
    result.add_argument('--runner-identity', required=True)
    return result


def main(argv=None):
    try:
        qualify(parser().parse_args(argv))
        return 0
    except (ValueError, OSError, UnicodeError, subprocess.SubprocessError, KeyError, TypeError, AttributeError) as error:
        emit({'type': 'error', 'command': 'native-install', 'state': 'blocked', 'publication_allowed': False, 'error': str(error)[:2000]})
        return 2


if __name__ == '__main__':
    sys.exit(main())
