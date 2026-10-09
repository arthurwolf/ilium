#!/usr/bin/env python3
"""Publish the exact qualified GitHub release assets as one GHCR OCI artifact.

The OCI package is an asset bundle, not a runnable container. A version tag is
never replaced: an existing manifest must reconcile with the qualified bytes.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request

import release_pipeline as pipeline
import release_tool

REPOSITORY = 'arthurwolf/ilium'
PACKAGE = 'ghcr.io/arthurwolf/ilium-release'
SOURCE = 'https://github.com/' + REPOSITORY
ARTIFACT_TYPE = 'application/vnd.ilium.release.config.v1+json'
LAYER_TYPE = 'application/octet-stream'
MANIFEST_TYPE = 'application/vnd.oci.image.manifest.v1+json'
PACKAGE_API = 'https://api.github.com/users/arthurwolf/packages/container/ilium-release'


def require(condition, message):
    if not condition:
        raise release_tool.ReleaseError(message)


def emit(kind, **fields):
    release_tool.emit({'type': kind, 'command': 'publish-ghcr', **fields})


def prepare(arguments, environment):
    # These checks precede registry credentials and all network mutation.
    require(environment.get('GITHUB_EVENT_NAME') == 'push', 'GHCR publication requires a tag push; dispatch cannot publish')
    require(environment.get('GITHUB_REPOSITORY') == REPOSITORY, 'GHCR publication repository differs')
    metadata, targets = pipeline.candidate_data(arguments.candidate, arguments.manifest, arguments.workspace)
    require(len(targets) == 5, 'GHCR publication requires all five native targets')
    tag = metadata['tag']
    require(environment.get('GITHUB_REF') == 'refs/tags/' + tag and
            environment.get('GITHUB_SHA') == metadata['commit'],
            'GHCR publication tag or commit differs from qualified source')
    require(re.fullmatch(r'v[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?', tag),
            'GHCR tag has unexpected syntax')
    qualification_file = arguments.candidate / 'qualification.json'
    qualification = release_tool.read_json(qualification_file)
    require(qualification.get('schema') == 1 and qualification.get('state') == 'passed' and
            qualification.get('publication_allowed') is True and
            qualification.get('tag') == tag and qualification.get('commit') == metadata['commit'] and
            qualification.get('archives') == metadata['archives'] and
            qualification.get('public_transport_verified') is False,
            'GHCR candidate is unqualified or does not contain the local five-target receipt')
    require(qualification_file.read_bytes() == arguments.final_qualification.read_bytes(),
            'GHCR final independent qualification differs from candidate qualification')
    files = pipeline.publication_files(arguments.candidate)
    assets = pipeline.asset_hashes(files)
    receipt = release_tool.read_json(arguments.publication_receipt)
    require(receipt.get('schema') == 1 and receipt.get('state') == 'passed' and
            receipt.get('immutable') is True and receipt.get('tag') == tag and
            receipt.get('commit') == metadata['commit'] and receipt.get('assets') == assets and
            isinstance(receipt.get('release_id'), int),
            'GHCR payload differs from the published immutable GitHub release')
    completed = release_tool.read_json(arguments.completed_qualification)
    require(completed.get('schema') == 1 and completed.get('state') == 'passed' and
            completed.get('publication_allowed') is True and
            completed.get('public_transport_verified') is True and
            completed.get('tag') == tag and completed.get('commit') == metadata['commit'] and
            completed.get('archives') == metadata['archives'],
            'GHCR publication requires all five completed public installation receipts')
    channel = release_tool.read_json(arguments.channel_readback)
    require(channel.get('schema') == 1 and channel.get('state') == 'passed' and
            channel.get('immutable') is True and channel.get('tag') == tag and
            channel.get('commit') == metadata['commit'] and
            channel.get('latest_release_id') == receipt['release_id'] and
            isinstance(channel.get('production_deployment_id'), str) and
            channel['production_deployment_id'],
            'GHCR publication requires final latest/assets/production readback')
    require(environment.get('GHCR_TOKEN') and environment.get('GITHUB_ACTOR'),
            'GHCR publisher lacks its scoped Actions credential or actor')
    return {'reference': PACKAGE + ':' + tag, 'tag': tag, 'commit': metadata['commit'],
            'files': files, 'assets': assets, 'release_id': receipt['release_id']}


def validate_manifest(manifest, plan):
    config = manifest.get('config') if isinstance(manifest, dict) else None
    require(isinstance(manifest, dict) and manifest.get('schemaVersion') == 2 and
            manifest.get('mediaType') == MANIFEST_TYPE and
            isinstance(config, dict) and config.get('mediaType') == ARTIFACT_TYPE,
            'existing GHCR version is not the Ilium release artifact')
    annotations = manifest.get('annotations')
    require(isinstance(annotations, dict) and annotations.get('org.opencontainers.image.source') == SOURCE and
            annotations.get('org.opencontainers.image.version') == plan['tag'],
            'existing GHCR version has different source or version metadata')
    layers = manifest.get('layers')
    require(isinstance(layers, list) and len(layers) == len(plan['files']),
            'existing GHCR version has a different asset count')
    actual = {}
    for layer in layers:
        require(isinstance(layer, dict), 'existing GHCR layer is malformed')
        title = layer.get('annotations')
        name = title.get('org.opencontainers.image.title') if isinstance(title, dict) else None
        require(isinstance(name, str) and name not in actual and
                re.fullmatch(r'[A-Za-z0-9_.-]+', name),
                'existing GHCR layer name is unsafe or repeated')
        require(layer.get('mediaType') == LAYER_TYPE and
                type(layer.get('size')) is int and layer['size'] > 0 and
                layer.get('digest') == 'sha256:' + plan['assets'].get(name, ''),
                'existing GHCR layer bytes differ: ' + name)
        actual[name] = layer['size']
    require(actual == {path.name: path.stat().st_size for path in plan['files']},
            'existing GHCR asset names or sizes differ')


def validate_public_files(directory, plan):
    directory = Path(directory)
    paths = list(directory.rglob('*'))
    require(all(path.is_file() and not path.is_symlink() and path.parent == directory
                for path in paths), 'anonymous GHCR pull contains an unsafe or nested entry')
    require({path.name for path in paths} == set(plan['assets']),
            'anonymous GHCR pull asset inventory differs')
    for path in paths:
        require(pipeline.sha(path) == plan['assets'][path.name],
                'anonymous GHCR pull has a SHA-256 mismatch: ' + path.name)


def validate_package_metadata(metadata):
    require(isinstance(metadata, dict), 'public GitHub package metadata is malformed')
    require(metadata.get('name') == 'ilium-release' and
            metadata.get('package_type') == 'container' and
            metadata.get('visibility') == 'public',
            'GitHub package entry is absent or not public')
    repository = metadata.get('repository')
    require(isinstance(repository, dict) and repository.get('full_name') == REPOSITORY,
            'GitHub package entry is not linked to the Ilium repository')
    owner = metadata.get('owner')
    require(owner is None or (isinstance(owner, dict) and owner.get('login') == 'arthurwolf'),
            'GitHub package entry has a different owner')
    url = metadata.get('html_url')
    require(isinstance(url, str), 'GitHub package entry has no package page')
    page = urllib.parse.urlsplit(url)
    package_id = metadata.get('id')
    user_page = '/users/arthurwolf/packages/container/package/ilium-release'
    linked_page = (f'/arthurwolf/ilium/packages/{package_id}'
                   if type(package_id) is int and package_id > 0 else None)
    require(page.scheme == 'https' and page.netloc == 'github.com' and
            not page.query and not page.fragment and page.path in (user_page, linked_page),
            'GitHub package entry points to an unexpected package page')
    return url


class OrasClient:
    def __init__(self, executable, actor, token, scratch):
        self.executable = executable
        self.actor = actor
        self.token = token
        self.scratch = Path(scratch)
        self.auth_config = self.scratch / 'auth.json'
        self.public_config = self.scratch / 'public.json'
        self.public_config.write_text('{"auths":{}}\n', encoding='utf-8')
        self.public_config.chmod(0o600)
        self.docker_config = self.scratch / 'docker-config'
        self.docker_config.mkdir(mode=0o700)
        self.environment = os.environ.copy()
        self.environment.pop('GHCR_TOKEN', None)
        self.environment['DOCKER_CONFIG'] = str(self.docker_config)
        self.environment['ORAS_CACHE'] = str(self.scratch / 'oras-cache')

    def run(self, args, *, public=False, input_text=None, cwd=None, timeout=300):
        command = [self.executable, *args, '--registry-config',
                   str(self.public_config if public else self.auth_config)]
        try:
            return subprocess.run(command, input=input_text, text=True,
                                  capture_output=True, cwd=cwd, env=self.environment,
                                  timeout=timeout, check=False)
        except (OSError, subprocess.TimeoutExpired) as error:
            raise release_tool.ReleaseError('ORAS operation could not complete: ' + type(error).__name__) from None

    def checked(self, args, *, public=False, cwd=None, timeout=300):
        result = self.run(args, public=public, cwd=cwd, timeout=timeout)
        if result.returncode:
            detail = (result.stderr or result.stdout).replace(self.token, '[redacted]')[-700:].strip()
            raise release_tool.ReleaseError('ORAS ' + args[0] + ' failed: ' + detail)
        return result.stdout

    def login(self):
        result = self.run(['login', 'ghcr.io', '--username', self.actor,
                           '--password-stdin'], input_text=self.token + '\n')
        require(result.returncode == 0 and self.auth_config.is_file(),
                'ORAS GHCR login failed')
        self.auth_config.chmod(0o600)

    def fetch(self, reference, *, public=False):
        # --output preserves the exact OCI manifest bytes for digest verification.
        with tempfile.TemporaryDirectory(prefix='manifest-', dir=self.scratch) as temporary:
            output = Path(temporary) / 'manifest.json'
            result = self.run(['manifest', 'fetch', '--output', str(output), reference], public=public)
            if result.returncode:
                error = (result.stderr or result.stdout).lower()
                denied = ('unauthorized', 'denied', 'forbidden', 'authentication', '401', '403')
                missing = ('manifest unknown', 'name unknown', 'not found', '404')
                if not public and not any(word in error for word in denied) and any(word in error for word in missing):
                    return None
                raise release_tool.ReleaseError('GHCR manifest read failed; access or registry state is unresolved')
            require(output.is_file(), 'GHCR manifest fetch did not write raw bytes')
            raw = output.read_bytes()
            if '@' in reference:
                expected = reference.rsplit('@', 1)[1]
                require('sha256:' + hashlib.sha256(raw).hexdigest() == expected,
                        'GHCR digest manifest bytes differ from requested digest')
            try:
                return json.loads(raw)
            except json.JSONDecodeError as error:
                raise release_tool.ReleaseError('GHCR manifest response is not JSON') from error

    def resolve(self, reference, *, public=False):
        value = self.checked(['resolve', reference], public=public).strip()
        require(re.fullmatch(r'sha256:[0-9a-f]{64}', value), 'GHCR resolved manifest digest is invalid')
        return value

    def push(self, plan):
        names = [path.name + ':' + LAYER_TYPE for path in plan['files']]
        self.checked(['push', '--image-spec', 'v1.0', '--artifact-type', ARTIFACT_TYPE,
                      '--annotation', 'org.opencontainers.image.source=' + SOURCE,
                      '--annotation', 'org.opencontainers.image.version=' + plan['tag'],
                      '--concurrency', '4', '--no-tty', plan['reference'], *names],
                     cwd=plan['files'][0].parent, timeout=5400)

    def pull_public(self, reference, directory):
        self.checked(['pull', '--no-tty', '--output', str(directory), reference],
                     public=True, timeout=5400)


def public_package_metadata(token):
    request = urllib.request.Request(PACKAGE_API, headers={
        'Accept': 'application/vnd.github+json',
        'X-GitHub-Api-Version': '2022-11-28', 'User-Agent': 'ilium-release-publisher',
        'Authorization': 'Bearer ' + token})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            require(response.status == 200, 'public GitHub package metadata did not return 200')
            return json.load(response)
    except (urllib.error.URLError, ValueError):
        # HTTPError may include request headers, so no exception chain reaches JSONL.
        raise release_tool.ReleaseError('public GitHub package metadata is unavailable') from None


def write_intent(path, plan):
    """Persist the exact qualified write identity before touching a registry tag."""
    path = Path(path)
    intent = {'schema': 1, 'state': 'prepared', 'operation': 'publish-ghcr',
              'repository': REPOSITORY, 'package': PACKAGE, 'source': SOURCE,
              'reference': plan['reference'], 'tag': plan['tag'],
              'commit': plan['commit'], 'release_id': plan['release_id'],
              'assets': plan['assets']}
    path.parent.mkdir(parents=True, exist_ok=True)
    try:
        with path.open('x', encoding='utf-8') as destination:
            json.dump(intent, destination, sort_keys=True, indent=2)
            destination.write('\n')
            destination.flush()
            os.fsync(destination.fileno())
        # Windows cannot open directories through os.open. The production
        # publisher runs on Linux and retains the directory durability fence.
        if sys.platform != 'win32':
            directory = os.open(path.parent, os.O_RDONLY)
            try:
                os.fsync(directory)
            finally:
                os.close(directory)
    except FileExistsError:
        require(release_tool.read_json(path) == intent,
                'existing GHCR publication intent differs from qualified payload')
    return intent


def publish(plan, client, output, metadata_reader=None, *, intent_path=None):
    if intent_path is None:
        intent_path = Path(output).with_name('ghcr-intent.json')
    require(Path(intent_path).resolve() != Path(output).resolve(),
            'GHCR intent and final receipt must use distinct paths')
    require(pipeline.asset_hashes(plan['files']) == plan['assets'],
            'qualified local payload changed before GHCR publication intent')
    write_intent(intent_path, plan)
    emit('artifact', role='publication-intent', path=str(Path(intent_path).resolve()),
         reference=plan['reference'], assets=len(plan['assets']))
    existing = client.fetch(plan['reference'])
    state = 'existing-verified' if existing is not None else 'new'
    if existing is not None:
        validate_manifest(existing, plan)  # Refuse to replace another owner's tag.
    else:
        emit('progress', state='publishing', reference=plan['reference'], assets=len(plan['assets']))
        try:
            client.push(plan)
        except release_tool.ReleaseError:
            # A transport error can occur after GHCR accepts a complete manifest.
            recovered = client.fetch(plan['reference'])
            if recovered is None:
                raise
            validate_manifest(recovered, plan)
            state = 'recovered-after-uncertain-push'
    # A tag can move between requests. All content validation and pulling uses
    # the immutable digest selected here; final tag checks detect later drift.
    digest = client.resolve(plan['reference'])
    immutable_reference = PACKAGE + '@' + digest
    manifest = client.fetch(immutable_reference)
    require(manifest is not None, 'GHCR digest disappeared after publication')
    validate_manifest(manifest, plan)
    require(pipeline.asset_hashes(plan['files']) == plan['assets'],
            'qualified local payload changed during GHCR upload')
    try:
        public_manifest = client.fetch(immutable_reference, public=True)
    except release_tool.ReleaseError:
        # The first GHCR upload creates a private package by default. Keep the
        # verified upload identity even when visibility or transport is unresolved.
        emit('warning', state='public-verification-pending',
             reference=plan['reference'], manifest_digest=digest,
             intent_path=str(Path(intent_path).resolve()),
             message='Authenticated bundle verified; anonymous access failed. Check package visibility and registry availability, then retry the same qualified tag without replacing its assets.')
        raise
    require(public_manifest is not None, 'anonymous GHCR digest manifest is absent')
    validate_manifest(public_manifest, plan)
    require(client.resolve(immutable_reference, public=True) == digest,
            'anonymous GHCR digest reference resolved to another manifest')
    with tempfile.TemporaryDirectory(prefix='ilium-ghcr-public-') as pulled:
        client.pull_public(immutable_reference, pulled)  # Uses an empty registry config.
        validate_public_files(pulled, plan)
    require(client.resolve(plan['reference'], public=True) == digest,
            'anonymous GHCR manifest digest differs from authenticated readback')
    metadata = public_package_metadata(client.token) if metadata_reader is None else metadata_reader()
    url = validate_package_metadata(metadata)
    require(client.resolve(plan['reference']) == digest,
            'GHCR tag moved during digest-bound verification')
    receipt = {'schema': 1, 'state': 'passed', 'tag': plan['tag'], 'commit': plan['commit'],
               'release_id': plan['release_id'], 'reference': plan['reference'],
               'manifest_digest': digest, 'assets': plan['assets'],
               'anonymous_pull_verified': True, 'package_visibility': 'public',
               'linked_repository': REPOSITORY, 'package_url': url, 'retry_state': state}
    pipeline.write_json(output, receipt)
    emit('result', state='passed', reference=plan['reference'], manifest_digest=digest,
         assets=len(plan['assets']), retry_state=state, output=str(output.resolve()))
    return receipt


def parser():
    result = release_tool.JsonArgumentParser(description=__doc__, allow_abbrev=False)
    result.add_argument('--candidate', required=True, type=Path)
    result.add_argument('--final-qualification', required=True, type=Path)
    result.add_argument('--publication-receipt', required=True, type=Path)
    result.add_argument('--completed-qualification', required=True, type=Path)
    result.add_argument('--channel-readback', required=True, type=Path)
    result.add_argument('--manifest', default=pipeline.ROOT / 'release/targets.toml', type=Path)
    result.add_argument('--workspace', default=pipeline.ROOT / 'Cargo.toml', type=Path)
    result.add_argument('--output', required=True, type=Path)
    result.add_argument('--intent', required=True, type=Path)
    result.add_argument('--oras', default='oras')
    return result


def main(argv=None):
    try:
        arguments = parser().parse_args(argv)
        plan = prepare(arguments, os.environ)
        with tempfile.TemporaryDirectory(prefix='ilium-ghcr-auth-') as scratch:
            client = OrasClient(arguments.oras, os.environ['GITHUB_ACTOR'],
                                os.environ['GHCR_TOKEN'], scratch)
            client.login()
            publish(plan, client, arguments.output, intent_path=arguments.intent)
        return 0
    except (ValueError, OSError, KeyError, TypeError) as error:
        emit('error', message=str(error)[:1200])
        return 1


if __name__ == '__main__':
    sys.exit(main())
