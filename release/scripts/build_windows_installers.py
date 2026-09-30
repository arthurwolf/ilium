#!/usr/bin/env python3
"""Build and smoke-test the Windows MSI and setup EXE from the audited ZIP.

Both installers are per-user (no elevation), install the audited files flat
under %LOCALAPPDATA%\\Programs\\ilium and append that directory to the user
PATH. They repackage the exact bytes of the native ZIP; `release_pipeline.py
aggregate` refuses a receipt whose file hashes differ from the native audit.
Stdout is JSONL. `render` runs anywhere; `build` needs `wix` and `iscc`;
`smoke` installs, runs and uninstalls both packages on a disposable Windows host.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
import release_tool

MSI_NAME = 'ilium-windows-x86_64.msi'
EXE_NAME = 'ilium-windows-x86_64-setup.exe'
INSTALLER_NAMES = (EXE_NAME, MSI_NAME)
RECEIPT_NAME = 'windows-installers.json'
ZIP_PREFIX = 'ilium-windows-x86_64'
PUBLISHER = 'Arthur Wolf'
HOMEPAGE = 'https://github.com/arthurwolf/ilium'
# Stable identities: changing either breaks upgrade and uninstall of older builds.
MSI_UPGRADE_CODE = '6F1B7D0E-2C54-4A9B-9E3D-51C8A4B7F0D2'
MSI_COMPONENT_CODE = 'A4D07E19-5B83-4C62-B0F1-7E2C9D3A8F46'
EXE_APP_ID = '{{C3A92E58-7B16-4D0F-8A41-0E5D9F2B6C73}'
MEMBER_PATTERN = re.compile(r'\A(?:ilium\.exe|ilium-server\.exe|VERSION|THIRD-PARTY\.txt|[A-Za-z0-9_-]+\.dll)\Z')
VERSION_PATTERN = re.compile(r'\A(?:0|[1-9][0-9]{0,4})\.(?:0|[1-9][0-9]{0,4})\.(?:0|[1-9][0-9]{0,4})\Z')
MAX_MEMBER_BYTES = 1_073_741_824
MAX_TOTAL_BYTES = 2_000_000_000


def require(condition, message):
    if not condition:
        raise release_tool.ReleaseError(message)


def emit(kind, **values):
    release_tool.emit({'type': kind, **values})


def sha(path):
    value = hashlib.sha256()
    with Path(path).open('rb') as source:
        while block := source.read(1024 * 1024):
            value.update(block)
    return value.hexdigest()


def xml_attribute(value):
    return str(value).replace('&', '&amp;').replace('"', '&quot;').replace('<', '&lt;').replace('>', '&gt;')


def version_from_tag(tag):
    require(isinstance(tag, str) and tag.startswith('v') and VERSION_PATTERN.fullmatch(tag[1:]),
            'installer versions must be plain vMAJOR.MINOR.PATCH (MSI cannot carry a pre-release suffix)')
    major, minor, patch = (int(part) for part in tag[1:].split('.'))
    require(major <= 255 and minor <= 255 and patch <= 65535, 'MSI ProductVersion limits are 255.255.65535')
    return tag[1:]


def extract_package(archive, version, destination):
    """Flatten the validated ZIP into a new directory; returns {name: sha256}."""
    archive, destination = Path(archive), Path(destination)
    require(archive.is_file() and not archive.is_symlink(), 'package archive must be a regular file')
    require(not destination.exists(), 'package directory must be new')
    destination.mkdir(parents=True)
    seen, total = set(), 0
    with zipfile.ZipFile(archive) as package:
        entries = package.infolist()
        require(0 < len(entries) <= 128, 'unexpected ZIP member count')
        for entry in entries:
            if entry.filename == ZIP_PREFIX + '/':
                continue
            require(entry.filename.startswith(ZIP_PREFIX + '/') and not entry.is_dir(), 'unexpected ZIP path: ' + entry.filename)
            name = entry.filename[len(ZIP_PREFIX) + 1:]
            require(MEMBER_PATTERN.fullmatch(name) and name not in seen, 'unsafe, duplicate or unexpected ZIP member: ' + name)
            require(0 < entry.file_size <= MAX_MEMBER_BYTES, 'ZIP member size exceeds bound: ' + name)
            total += entry.file_size
            require(total <= MAX_TOTAL_BYTES, 'ZIP expanded size exceeds bound')
            seen.add(name)
            (destination / name).write_bytes(package.read(entry))
    require({'ilium.exe', 'ilium-server.exe', 'VERSION', 'THIRD-PARTY.txt'} <= seen, 'ZIP is missing a required member')
    require((destination / 'VERSION').read_bytes() == (version + '\n').encode(), 'ZIP VERSION differs from the release version')
    return {name: sha(destination / name) for name in sorted(seen)}


def render_wix(package_directory, files, version):
    rows = ''.join(
        '        <File Id="file_%d" Source="%s" />\n' % (index, xml_attribute(Path(package_directory) / name))
        for index, name in enumerate(sorted(files)))
    return f'''<?xml version="1.0" encoding="utf-8"?>
<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs">
  <Package Name="Ilium" Manufacturer="{xml_attribute(PUBLISHER)}" Version="{version}" UpgradeCode="{MSI_UPGRADE_CODE}" Scope="perUser" Language="1033" InstallerVersion="500" Compressed="yes">
    <SummaryInformation Description="Ilium terminal workspace for AI agents" />
    <Property Id="ARPURLINFOABOUT" Value="{xml_attribute(HOMEPAGE)}" />
    <MajorUpgrade DowngradeErrorMessage="A newer version of Ilium is already installed." />
    <MediaTemplate EmbedCab="yes" CompressionLevel="high" />
    <StandardDirectory Id="LocalAppDataFolder">
      <Directory Id="IliumPrograms" Name="Programs">
        <Directory Id="INSTALLFOLDER" Name="ilium" />
      </Directory>
    </StandardDirectory>
    <Feature Id="Main" Title="Ilium" Level="1">
      <ComponentGroupRef Id="IliumFiles" />
    </Feature>
    <ComponentGroup Id="IliumFiles" Directory="INSTALLFOLDER">
      <Component Id="IliumPackage" Guid="{MSI_COMPONENT_CODE}">
{rows}        <RegistryValue Root="HKCU" Key="Software\\Ilium" Name="InstallDirectory" Type="string" Value="[INSTALLFOLDER]" KeyPath="yes" />
        <Environment Id="IliumPath" Name="PATH" Value="[INSTALLFOLDER]" Action="set" Part="last" System="no" />
        <RemoveFolder Id="RemoveIliumFolder" Directory="INSTALLFOLDER" On="uninstall" />
        <RemoveFolder Id="RemoveIliumPrograms" Directory="IliumPrograms" On="uninstall" />
      </Component>
    </ComponentGroup>
  </Package>
</Wix>
'''


INNO_TEMPLATE = r'''[Setup]
AppId=@APP_ID@
AppName=Ilium
AppVersion=@VERSION@
AppPublisher=@PUBLISHER@
AppPublisherURL=@HOMEPAGE@
DefaultDirName={localappdata}\Programs\ilium
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ChangesEnvironment=yes
UninstallDisplayName=Ilium
OutputDir=@OUTPUT_DIR@
OutputBaseFilename=@OUTPUT_BASE@
SourceDir=@SOURCE_DIR@

[Files]
Source: "*"; DestDir: "{app}"; Flags: ignoreversion

[Code]
const
  EnvironmentKey = 'Environment';

function ListContains(const List, Entry: string): Boolean;
begin
  Result := Pos(';' + Lowercase(Entry) + ';', ';' + Lowercase(List) + ';') > 0;
end;

procedure AddToUserPath(const Entry: string);
var
  Current: string;
begin
  if not RegQueryStringValue(HKCU, EnvironmentKey, 'Path', Current) then
    Current := '';
  if ListContains(Current, Entry) then
    Exit;
  if (Current <> '') and (Current[Length(Current)] <> ';') then
    Current := Current + ';';
  RegWriteExpandStringValue(HKCU, EnvironmentKey, 'Path', Current + Entry);
end;

procedure RemoveFromUserPath(const Entry: string);
var
  Current, Needle: string;
  Position: Integer;
begin
  if not RegQueryStringValue(HKCU, EnvironmentKey, 'Path', Current) then
    Exit;
  Current := ';' + Current + ';';
  Needle := ';' + Entry + ';';
  Position := Pos(Lowercase(Needle), Lowercase(Current));
  while Position > 0 do
  begin
    Delete(Current, Position, Length(Needle) - 1);
    Position := Pos(Lowercase(Needle), Lowercase(Current));
  end;
  if (Length(Current) > 0) and (Current[1] = ';') then
    Delete(Current, 1, 1);
  if (Length(Current) > 0) and (Current[Length(Current)] = ';') then
    Delete(Current, Length(Current), 1);
  RegWriteExpandStringValue(HKCU, EnvironmentKey, 'Path', Current);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    AddToUserPath(ExpandConstant('{app}'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RemoveFromUserPath(ExpandConstant('{app}'));
end;
'''


def render_inno(package_directory, output_directory, version):
    text = INNO_TEMPLATE
    for key, value in (('APP_ID', EXE_APP_ID), ('VERSION', version), ('PUBLISHER', PUBLISHER), ('HOMEPAGE', HOMEPAGE),
                       ('OUTPUT_DIR', str(output_directory)), ('OUTPUT_BASE', EXE_NAME[:-len('.exe')]),
                       ('SOURCE_DIR', str(package_directory))):
        text = text.replace('@' + key + '@', value)
    require('@' not in text.split('[Code]')[0], 'unresolved Inno Setup placeholder')
    return text


def find_tool(name, extra_candidates=()):
    found = shutil.which(name)
    if found:
        return found
    for candidate in extra_candidates:
        if candidate and Path(candidate).is_file():
            return str(candidate)
    raise release_tool.ReleaseError('required tool not found: ' + name)


def run(command, **options):
    emit('progress', command=[Path(command[0]).name, *command[1:3]])
    result = subprocess.run(command, capture_output=True, text=True, **options)
    if result.returncode != 0:
        raise release_tool.ReleaseError('%s exited %d: %s' % (Path(command[0]).name, result.returncode, (result.stdout + result.stderr)[-1200:]))
    return result


def build(arguments):
    version = version_from_tag(arguments.tag)
    work, output = arguments.work.resolve(), arguments.output.resolve()
    require(not output.exists(), 'installer output must be new')
    work.mkdir(parents=True, exist_ok=True)
    package_directory = work / 'package'
    files = extract_package(arguments.archive, version, package_directory)
    wix_source = work / 'ilium.wxs'
    wix_source.write_text(render_wix(package_directory, files, version), encoding='utf-8')
    output.mkdir(parents=True)
    wix = find_tool('wix')
    run([wix, 'build', '-arch', 'x64', '-o', str(output / MSI_NAME), str(wix_source)])
    inno_source = work / 'ilium.iss'
    inno_source.write_text(render_inno(package_directory, output, version), encoding='utf-8')
    iscc = find_tool('iscc', [os.path.join(os.environ.get('ProgramFiles(x86)', ''), 'Inno Setup 6', 'ISCC.exe'),
                              os.path.join(os.environ.get('ProgramFiles', ''), 'Inno Setup 6', 'ISCC.exe')])
    run([iscc, '/Qp', str(inno_source)])
    installers = {name: sha(output / name) for name in INSTALLER_NAMES}
    for name in INSTALLER_NAMES:
        require((output / name).stat().st_size > 0, 'installer is empty: ' + name)
    receipt = {'schema': 1, 'tag': arguments.tag, 'version': version, 'source_archive': arguments.archive.name,
               'source_archive_sha256': sha(arguments.archive), 'package_files': files, 'installers': installers}
    (output / RECEIPT_NAME).write_text(json.dumps(receipt, indent=2, sort_keys=True) + '\n', encoding='ascii')
    emit('result', command='build', state='built', output=str(output), receipt=str(output / RECEIPT_NAME), installers=installers)


def render(arguments):
    version = version_from_tag(arguments.tag)
    work = arguments.work.resolve()
    files = extract_package(arguments.archive, version, work / 'package')
    (work / 'ilium.wxs').write_text(render_wix(work / 'package', files, version), encoding='utf-8')
    (work / 'ilium.iss').write_text(render_inno(work / 'package', work / 'out', version), encoding='utf-8')
    emit('result', command='render', state='rendered', wix=str(work / 'ilium.wxs'), inno=str(work / 'ilium.iss'))


def user_path():
    import winreg
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, 'Environment') as key:
        try:
            return winreg.QueryValueEx(key, 'Path')[0]
        except FileNotFoundError:
            return ''


def path_has(directory):
    return directory.lower() in [item.strip().lower() for item in user_path().split(';')]


def wait_removed(directory, label):
    for _ in range(60):
        if not directory.exists():
            return
        time.sleep(1)
    raise release_tool.ReleaseError(label + ' uninstall left ' + str(directory))


def check_installed(directory, version, label):
    for name in ('ilium.exe', 'ilium-server.exe'):
        require((directory / name).is_file(), label + ' did not install ' + name)
    require(path_has(str(directory)), label + ' did not add the install directory to the user PATH')
    reported = subprocess.run([str(directory / 'ilium.exe'), '--version'], capture_output=True, text=True, timeout=60)
    require(reported.returncode == 0 and version in reported.stdout, label + ' installed client does not report ' + version)


def check_removed(directory, label):
    wait_removed(directory, label)
    require(not path_has(str(directory)), label + ' uninstall left the user PATH entry')


def smoke(arguments):
    require(os.name == 'nt', 'smoke runs only on Windows')
    version = version_from_tag(arguments.tag)
    installers = arguments.installers.resolve()
    directory = Path(os.environ['LOCALAPPDATA']) / 'Programs' / 'ilium'
    require(not directory.exists() and not path_has(str(directory)), 'smoke needs a clean account')
    log = arguments.log.resolve()
    log.mkdir(parents=True, exist_ok=True)
    msi = installers / MSI_NAME
    run(['msiexec', '/i', str(msi), '/qn', '/norestart', '/l*v', str(log / 'msi-install.log')])
    check_installed(directory, version, 'MSI')
    run(['msiexec', '/x', str(msi), '/qn', '/norestart', '/l*v', str(log / 'msi-uninstall.log')])
    check_removed(directory, 'MSI')
    setup = installers / EXE_NAME
    run([str(setup), '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/LOG=' + str(log / 'exe-install.log')])
    check_installed(directory, version, 'EXE')
    uninstaller = next(directory.glob('unins*.exe'), None)
    require(uninstaller is not None, 'EXE installer wrote no uninstaller')
    run([str(uninstaller), '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART'])
    check_removed(directory, 'EXE')
    emit('result', command='smoke', state='passed', version=version, installers={name: sha(installers / name) for name in INSTALLER_NAMES})


def parser():
    import argparse
    result = release_tool.JsonArgumentParser(description=__doc__, allow_abbrev=False)
    commands = result.add_subparsers(dest='command', required=True)
    for name in ('render', 'build'):
        command = commands.add_parser(name, allow_abbrev=False)
        command.add_argument('--tag', required=True)
        command.add_argument('--archive', type=Path, required=True)
        command.add_argument('--work', type=Path, required=True)
        if name == 'build':
            command.add_argument('--output', type=Path, required=True)
    command = commands.add_parser('smoke', allow_abbrev=False)
    command.add_argument('--tag', required=True)
    command.add_argument('--installers', type=Path, required=True)
    command.add_argument('--log', type=Path, required=True)
    return result


def main(argv=None):
    try:
        arguments = parser().parse_args(argv)
        {'render': render, 'build': build, 'smoke': smoke}[arguments.command](arguments)
        return 0
    except (ValueError, OSError, KeyError, zipfile.BadZipFile, subprocess.SubprocessError) as error:
        emit('error', message=str(error)[:1200])
        return 1


if __name__ == '__main__':
    sys.exit(main())
