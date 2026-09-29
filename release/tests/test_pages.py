"""Isolated static-site contracts; no provider or production network access."""
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import urllib.error
import urllib.request
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'release/scripts'))
import release_tool
import pages


class PagesTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'release/site').mkdir(parents=True)
        shutil.copytree(ROOT / 'release/site', self.root / 'release/site', dirs_exist_ok=True)
        self.manifest = self.root / 'release/targets.toml'
        shutil.copyfile(ROOT / 'release/targets.toml', self.manifest)
        self.targets = release_tool.load_targets(self.manifest)
        # Synthetic installer bodies stay entirely in this isolated fixture.
        (self.root / 'release/install.sh').write_text('#!/bin/sh\n' + release_tool.posix_table(self.targets) + '\n')
        (self.root / 'release/install.ps1').write_text(pages.windows_table(self.targets) + '\n')
        self.sums = self.root / 'SHA256SUMS'
        self.valid_sums = ''.join('a' * 64 + '  ' + row['archive'] + '\n' for row in self.targets)
        self.sums.write_text(self.valid_sums)
        self.output = self.root / 'pages'

    def build(self):
        return pages.build_pages(self.manifest, self.output, 'v0.1.0', self.sums, source_root=self.root)

    def test_exact_inventory_copies_and_metadata(self):
        metadata = self.build()
        self.assertEqual(set(p.name for p in self.output.iterdir()), pages.FILES)
        for name in ('install.sh', 'install.ps1'):
            self.assertEqual((self.output / name).read_bytes(), (self.root / 'release' / name).read_bytes())
        self.assertEqual(metadata['release_tag'], 'v0.1.0')
        self.assertEqual(metadata['targets'], self.targets)
        self.assertEqual(set(metadata['release_sha256sums']), {row['archive'] for row in self.targets})
        self.assertEqual(len(metadata['files']), 5)
        pages.verify_pages(self.output, self.sums, source_root=self.root)

    def test_literal_commands_targets_and_provenance(self):
        self.build()
        text = (self.output / 'index.html').read_text()
        self.assertIn("curl --proto '=https' --tlsv1.2 -LsSf https://ilium-setup.pages.dev/install.sh | sh", text)
        self.assertIn('irm https://ilium-setup.pages.dev/install.ps1 | iex', text)
        for row in self.targets:
            self.assertIn(row['rust_target'], text)
            self.assertIn(row['minimum_tested_os'], text)
        for link in ('/install.sh', '/install.ps1', '/manifest.json', '/releases/tag/v0.1.0', '/releases/download/v0.1.0/SHA256SUMS', '/attestations'):
            self.assertIn(link, text)
        self.assertNotIn('@', text)
        self.assertNotRegex(text.lower(), r'<script|<iframe|<link|<img|serviceworker|analytics')

    def test_no_store_mime_csp_and_custom_404(self):
        self.build()
        headers = pages.parse_headers((self.output / '_headers').read_text())
        for path in ('/', '/install.sh', '/install.ps1', '/manifest.json', '/missing'):
            merged = dict(headers['/*']); merged.update(headers.get(path, {}))
            self.assertIn('no-store', merged['Cache-Control'])
            self.assertEqual(merged['X-Content-Type-Options'], 'nosniff')
            self.assertIn("default-src 'none'", merged['Content-Security-Policy'])
            self.assertIn("frame-ancestors 'none'", merged['Content-Security-Policy'])
        self.assertEqual(headers['/install.sh']['Content-Type'], 'text/plain; charset=utf-8')
        self.assertEqual(headers['/install.ps1']['Content-Type'], 'text/plain; charset=utf-8')
        self.assertEqual(headers['/manifest.json']['Content-Type'], 'application/json; charset=utf-8')
        self.assertIn('Page not found', (self.output / '404.html').read_text())
        self.assertIn('href="/"', (self.output / '404.html').read_text())

    def test_concurrent_output_file_is_preserved(self):
        original_mkdir = Path.mkdir
        output = self.output

        def conflicting_mkdir(path, *arguments, **keywords):
            result = original_mkdir(path, *arguments, **keywords)
            if path == output:
                (path / '404.html').write_bytes(b'concurrent owner')
            return result

        with patch.object(Path, 'mkdir', conflicting_mkdir):
            with self.assertRaises(FileExistsError):
                self.build()
        self.assertEqual((output / '404.html').read_bytes(), b'concurrent owner')
        self.assertEqual(list(self.root.glob('.ilium-pages-*')), [])

    def test_manifest_not_the_approved_five_is_rejected(self):
        text = self.manifest.read_text()
        self.manifest.write_text(text[:text.rfind('[[target]]')])
        with self.assertRaises(ValueError):
            self.build()
        self.assertFalse(self.output.exists())

    def test_reject_invalid_tags(self):
        for tag in ('../v0.1.0', 'v0.1.0\n', 'v0.1', 'latest', 'v0.1.0<script>'):
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                pages.build_pages(self.manifest, self.output, tag, self.sums, source_root=self.root)
        self.assertFalse(self.output.exists())

    def test_reject_checksum_inventory_and_syntax(self):
        for invalid in (self.valid_sums + self.valid_sums.splitlines()[0] + '\n', self.valid_sums.splitlines()[0] + '\n', self.valid_sums + 'a' * 64 + '  extra.zip\n', self.valid_sums.replace('a' * 64, 'G' * 64), self.valid_sums.replace('  ilium-', '  ../ilium-')):
            with self.subTest(invalid=invalid[:80]):
                self.sums.write_text(invalid)
                with self.assertRaises(ValueError):
                    self.build()
        self.assertFalse(self.output.exists())

    def test_reject_missing_or_stale_installer(self):
        path = self.root / 'release/install.ps1'
        original = path.read_bytes()
        path.unlink()
        with self.assertRaises((ValueError, OSError)):
            self.build()
        path.write_bytes(original.replace(b'ilium-windows-x86_64.zip', b'ilium-windows-arm64.zip'))
        with self.assertRaises(ValueError):
            self.build()
        path.write_bytes(original)
        path = self.root / 'release/install.sh'
        path.write_bytes(path.read_bytes().replace(b'Linux/x86_64', b'Linux/armv7'))
        with self.assertRaises(ValueError):
            self.build()
        self.assertFalse(self.output.exists())

    def test_refuse_existing_output_and_unexpected_sources(self):
        self.output.mkdir()
        retained = self.output / 'user.txt'; retained.write_text('preserve')
        with self.assertRaises(ValueError):
            self.build()
        self.assertEqual(retained.read_text(), 'preserve')
        retained.unlink()
        (self.root / 'release/site/extra.js').write_text('extra')
        with self.assertRaises(ValueError):
            self.build()
        self.assertEqual(list(self.output.iterdir()), [])

    def test_reject_unsafe_static_content_and_secrets(self):
        source = self.root / 'release/site/index.html'
        original = source.read_bytes()
        for payload in (b'<script src="https://external.example/a.js"></script>', b'\x00binary', b'ghp_' + b'a' * 36, b'CLOUDFLARE_API_TOKEN=private-secret-value'):
            with self.subTest(payload=payload[:30]):
                source.write_bytes(original + payload)
                with self.assertRaises(ValueError):
                    self.build()
        self.assertFalse(self.output.exists())

    def test_verification_detects_tampering_missing_extra_and_drift(self):
        self.build()
        installer = self.output / 'install.sh'; original = installer.read_bytes()
        installer.write_bytes(original + b'echo changed\n')
        with self.assertRaises(ValueError):
            pages.verify_pages(self.output, self.sums, source_root=self.root)

        installer.write_bytes(original)
        (self.output / 'extra').write_text('x')
        with self.assertRaises(ValueError):
            pages.verify_pages(self.output, self.sums, source_root=self.root)
        (self.output / 'extra').unlink()
        metadata = json.loads((self.output / 'manifest.json').read_text())
        metadata['targets'][0]['archive'] = 'wrong.tar.gz'
        (self.output / 'manifest.json').write_text(json.dumps(metadata))
        with self.assertRaises(ValueError):
            pages.verify_pages(self.output, self.sums, source_root=self.root)

    def test_verification_rejects_forged_release_checksum_provenance(self):
        self.build()
        manifest_path = self.output / 'manifest.json'
        original = json.loads(manifest_path.read_text())
        for field in ('release_sha256sums_sha256', 'release_sha256sums'):
            with self.subTest(field=field):
                metadata = json.loads(json.dumps(original))
                if field == 'release_sha256sums_sha256':
                    metadata[field] = 'b' * 64
                else:
                    metadata[field] = {
                        archive: 'c' * 64 for archive in metadata[field]
                    }
                manifest_path.write_text(json.dumps(metadata))
                with self.assertRaises(ValueError):
                    pages.verify_pages(
                        self.output,
                        self.sums,
                        source_root=self.root,
                    )

    def test_reject_missing_source_wrong_headers_and_generic_404(self):
        for filename, replacements in (
            ('_headers', (('no-store, max-age=0', 'public, max-age=3600'), ('text/plain', 'application/octet-stream'), ("default-src 'none'", "default-src *"))),
            ('404.html', (('Page not found', 'Generic page'),)),
        ):
            source = self.root / 'release/site' / filename
            original = source.read_text()
            for before, after in replacements:
                with self.subTest(filename=filename, before=before):
                    source.write_text(original.replace(before, after))
                    with self.assertRaises(ValueError):
                        self.build()
            source.write_text(original)
            source.unlink()
            with self.assertRaises(ValueError):
                self.build()
            source.write_text(original)
        self.assertFalse(self.output.exists())

    def test_missing_output_canonical_drift_and_symlinks(self):
        self.build()
        source = self.root / 'release/install.ps1'
        source.write_bytes(source.read_bytes() + b'# Changed canonical bytes\n')
        with self.assertRaises(ValueError):
            pages.verify_pages(self.output, self.sums, source_root=self.root)
        missing = self.output / '404.html'
        missing.unlink()
        with self.assertRaises(ValueError):
            pages.verify_pages(self.output, self.sums, source_root=self.root)
        missing.symlink_to(self.root / 'release/site/404.html')
        with self.assertRaises(ValueError):
            pages.verify_pages(self.output, self.sums, source_root=self.root)

    def test_local_http_exact_bytes_headers_and_custom_missing_route(self):
        # This fixture interprets Pages _headers locally. It proves the static
        # contract and bytes, not Cloudflare's production deployment behavior.
        self.build()
        output = self.output
        headers = pages.parse_headers((output / '_headers').read_text())

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                route = self.path
                filename = 'index.html' if route == '/' else route.lstrip('/')
                found = filename in pages.FILES - {'_headers'}
                body = (output / (filename if found else '404.html')).read_bytes()
                self.send_response(200 if found else 404)
                merged = dict(headers['/*']); merged.update(headers.get(route, {}))
                merged.setdefault('Content-Type', 'text/html; charset=utf-8')
                for name, value in merged.items():
                    self.send_header(name, value)
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *_arguments):
                pass

        server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        server.daemon_threads = True
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            origin = 'http://127.0.0.1:' + str(server.server_port)
            for filename in pages.FILES - {'_headers'}:
                route = '/' if filename == 'index.html' else '/' + filename
                with urllib.request.urlopen(origin + route, timeout=3) as response:
                    self.assertEqual(response.status, 200)
                    self.assertEqual(response.read(), (output / filename).read_bytes())
                    self.assertIn('no-store', response.headers['Cache-Control'])
                    self.assertEqual(response.headers['X-Content-Type-Options'], 'nosniff')
                    self.assertIn("default-src 'none'", response.headers['Content-Security-Policy'])
                    if filename.endswith(('.sh', '.ps1')):
                        self.assertEqual(response.headers.get_content_type(), 'text/plain')
                    if filename == 'manifest.json':
                        self.assertEqual(response.headers.get_content_type(), 'application/json')
            with self.assertRaises(urllib.error.HTTPError) as error:
                urllib.request.urlopen(origin + '/missing', timeout=3)
            self.assertEqual(error.exception.code, 404)
            self.assertEqual(error.exception.read(), (output / '404.html').read_bytes())
            error.exception.close()
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=3)
        self.assertFalse(thread.is_alive())

    def test_cli_is_jsonl_on_errors_and_success(self):
        command = [sys.executable, str(ROOT / 'release/scripts/pages.py'), 'build-pages', '--manifest', str(self.manifest), '--output', str(self.output), '--release-tag', 'v0.1.0', '--release-sha256sums', str(self.sums), '--source-root', str(self.root)]
        result = subprocess.run(command, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        records = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual(records[-1]['type'], 'result')
        self.assertEqual(records[-1]['output'], str(self.output.resolve()))
        result = subprocess.run(command, capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(json.loads(result.stdout)['type'], 'error')
        self.assertEqual(result.stderr, '')

    def test_release_tool_verify_pages_cli_requires_candidate_checksums(self):
        output = self.root / 'canonical-pages'
        pages.build_pages(
            ROOT / 'release/targets.toml',
            output,
            'v0.1.0',
            self.sums,
            source_root=ROOT,
        )
        command = [
            sys.executable,
            str(ROOT / 'release/scripts/release_tool.py'),
            'verify-pages',
            '--manifest',
            str(ROOT / 'release/targets.toml'),
            '--directory',
            str(output),
            '--release-sha256sums',
            str(self.sums),
        ]
        result = subprocess.run(command, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        records = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual(records[-1]['type'], 'result')
        self.assertEqual(records[-1]['command'], 'verify-pages')

        self.sums.write_text(self.valid_sums.replace('a' * 64, 'b' * 64))
        result = subprocess.run(command, capture_output=True, text=True)
        self.assertEqual(result.returncode, 2, result.stderr + result.stdout)
        self.assertEqual(json.loads(result.stdout)['type'], 'error')
        self.assertEqual(result.stderr, '')


if __name__ == '__main__':
    unittest.main()
