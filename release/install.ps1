# Canonical per-user installer. Windows PowerShell 5.1 and current PowerShell.
# Usage: & ./install.ps1 [-Version V] [-InstallDir DIR] [-BinDir DIR]
#                          [-NoModifyPath] [-Uninstall]
# The default also works when this complete script is piped to iex. Release
# metadata is data, never source. No elevation, execution-policy or machine PATH changes.
[CmdletBinding()]
param([string]$Version = 'latest', [string]$InstallDir = '', [string]$BinDir = '',
      [switch]$NoModifyPath, [switch]$Uninstall)

function Test-IliumVersion([string]$Value) {
    return $Value -cmatch '\A[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?\z'
}

function Get-IliumTarget([string]$Architecture) {
# BEGIN GENERATED WINDOWS TARGETS
$checksum_archives = @('ilium-linux-x86_64.tar.gz', 'ilium-linux-aarch64.tar.gz', 'ilium-windows-x86_64.zip', 'ilium-macos-aarch64.tar.gz', 'ilium-macos-x86_64.tar.gz')
$windows_targets = @{
    'AMD64' = @{ archive = 'ilium-windows-x86_64.zip'; target = 'x86_64-pc-windows-msvc'; prefix = 'ilium-windows-x86_64' }
}
# END GENERATED WINDOWS TARGETS
    if (-not $windows_targets.ContainsKey($Architecture)) { throw 'Unsupported Windows architecture; supported: native AMD64 (x86-64).' }
    $selected = $windows_targets[$Architecture].Clone()
    $selected.checksum_archives = $checksum_archives
    return $selected
}

function Get-IliumNativeArchitecture {
    # Environment architecture is spoofable and x86/ARM emulation can advertise
    # AMD64. GetNativeSystemInfo is authoritative for the operating system.
    if (-not ('IliumInstaller.NativeSystem' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
namespace IliumInstaller {
    [StructLayout(LayoutKind.Sequential)]
    public struct SystemInfo {
        public ushort Architecture, Reserved;
        public uint PageSize;
        public IntPtr MinimumAddress, MaximumAddress;
        public UIntPtr ActiveMask;
        public uint ProcessorCount, ProcessorType, AllocationGranularity;
        public ushort ProcessorLevel, ProcessorRevision;
    }
    public static class NativeSystem {
        [DllImport("kernel32.dll")] public static extern void GetNativeSystemInfo(out SystemInfo information);
    }
}
'@
    }
    $information = New-Object IliumInstaller.SystemInfo
    [IliumInstaller.NativeSystem]::GetNativeSystemInfo([ref]$information)
    if ($information.Architecture -eq 9) { return 'AMD64' }
    if ($information.Architecture -eq 12) { return 'ARM64' }
    return 'x86'
}

function Assert-IliumPlainPath([string]$Path) {
    $cursor = [IO.Path]::GetFullPath($Path)
    while ($cursor) {
        if (Test-Path -LiteralPath $cursor) {
            if (([IO.File]::GetAttributes($cursor) -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "Unsafe reparse-point path: $cursor"
            }
        }
        $parent = [IO.Directory]::GetParent($cursor)
        if ($null -eq $parent) { break }
        $cursor = $parent.FullName
    }
}

function Get-IliumSafePath([string]$Path) {
    if ($Path -notmatch '\A[A-Za-z]:[\\/]' -or $Path -match '[\x00-\x1f\x7f;"%!]') {
        throw 'Use an absolute local drive path without controls, quotes, %, ! or semicolons.'
    }
    if ($Path -match '(?:\A|[\\/])\.{1,2}(?:[\\/]|\z)' -or $Path.Substring(2) -match ':' -or
        $Path -match '[ .](?:[\\/]|\z)' -or $Path -match '(?i)(?:[\\/])(?:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.[^\\/]*)?(?:[\\/]|\z)') { throw 'Unsafe path components.' }
    $full = [IO.Path]::GetFullPath($Path).TrimEnd('\')
    if ($full.Length -gt 160) { throw 'Install paths must be at most 160 characters for PowerShell 5.1.' }
    $allowed = @([Environment]::GetFolderPath('UserProfile'), [IO.Path]::GetTempPath())
    $inside = $false
    foreach ($base in $allowed) {
        $base = [IO.Path]::GetFullPath($base).TrimEnd('\') + '\'
        if ($full.StartsWith($base, [StringComparison]::OrdinalIgnoreCase)) { $inside = $true }
    }
    if (-not $inside) { throw 'Install directories must be below the current user profile or temporary directory.' }
    Assert-IliumPlainPath $full
    return $full
}

function New-IliumPrivateDirectory([string]$Path) {
    Assert-IliumPlainPath $Path
    $created = -not [IO.Directory]::Exists($Path)
    [IO.Directory]::CreateDirectory($Path) | Out-Null
    $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User
    if ($created) {
        $acl = New-Object Security.AccessControl.DirectorySecurity
        $acl.SetOwner($sid)
        $acl.SetAccessRuleProtection($true, $false)
        $rule = New-Object Security.AccessControl.FileSystemAccessRule($sid, 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow')
        $acl.AddAccessRule($rule)
        Set-Acl -LiteralPath $Path -AclObject $acl -ErrorAction Stop
    }
    $actual = Get-Acl -LiteralPath $Path -ErrorAction Stop
    if ($actual.GetOwner([Security.Principal.SecurityIdentifier]).Value -cne $sid.Value) { throw 'Install directory is not owned by the current user.' }
    $trusted = @($sid.Value, 'S-1-5-18', 'S-1-5-32-544')
    $writeMask = [Security.AccessControl.FileSystemRights]::Write -bor [Security.AccessControl.FileSystemRights]::Delete -bor [Security.AccessControl.FileSystemRights]::DeleteSubdirectoriesAndFiles -bor [Security.AccessControl.FileSystemRights]::ChangePermissions -bor [Security.AccessControl.FileSystemRights]::TakeOwnership
    foreach ($rule in $actual.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier])) {
        if ($rule.AccessControlType -eq 'Allow' -and ($rule.FileSystemRights -band $writeMask) -ne 0 -and $rule.IdentityReference.Value -cnotin $trusted) { throw 'Install directory grants unrelated principals write access; use a private per-user directory.' }
    }
}

function Write-IliumAtomic([string]$Path, [string]$Content, [switch]$NoReplace) {
    Assert-IliumPlainPath $Path
    $temporary = $Path + '.tmp-' + [guid]::NewGuid().ToString('N')
    try {
        $bytes = (New-Object Text.UTF8Encoding($false)).GetBytes($Content)
        $stream = [IO.File]::Open($temporary, 'CreateNew', 'Write', 'None')
        try { $stream.Write($bytes, 0, $bytes.Length); $stream.Flush($true) } finally { $stream.Dispose() }
        if (-not $NoReplace -and [IO.File]::Exists($Path)) { [IO.File]::Replace($temporary, $Path, [NullString]::Value) }
        else { [IO.File]::Move($temporary, $Path) }
    } finally { if ([IO.File]::Exists($temporary)) { [IO.File]::Delete($temporary) } }
}

function Get-IliumHash([string]$Path) {
    Assert-IliumPlainPath $Path
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256 -ErrorAction Stop).Hash.ToLowerInvariant()
}

function Get-IliumFreeSpace([string]$Root) {
    $drive = New-Object IO.DriveInfo([IO.Path]::GetPathRoot($Root))
    return $drive.AvailableFreeSpace
}

function Get-IliumUserPath { return [Environment]::GetEnvironmentVariable('Path', 'User') }
function Set-IliumUserPath([AllowNull()][string]$Value) { [Environment]::SetEnvironmentVariable('Path', $Value, 'User') }
function Test-IliumPathEntry([string]$Entry, [string]$Bin) {
    if (-not $Entry) { return $false }
    $expanded = [Environment]::ExpandEnvironmentVariables($Entry.Trim().Trim('"')).TrimEnd('\', '/')
    try { return [IO.Path]::GetFullPath($expanded).Equals($Bin, [StringComparison]::OrdinalIgnoreCase) }
    catch { return $false }
}
function Remove-IliumPathEntry([AllowNull()][string]$Value, [string]$Bin) {
    if ($null -eq $Value) { return $null }
    # Remove only the literal entry this installer inserted. Equivalent authored
    # entries (case, quoting, variables, trailing separators) remain authored.
    $parts = New-Object 'Collections.Generic.List[string]'
    foreach ($part in $Value.Split(';')) { $parts.Add($part) }
    for ($index = $parts.Count - 1; $index -ge 0; $index--) {
        if ($parts[$index] -ceq $Bin) { $parts.RemoveAt($index); break }
    }
    return ($parts -join ';')
}

function Save-IliumDownload([string]$Url, [string]$Destination) {
    $uri = [Uri]$Url
    for ($redirect = 0; $redirect -le 5; $redirect++) {
        if ($uri.Scheme -cne 'https' -or $uri.UserInfo -or $uri.Port -ne 443 -or
            $uri.Host -notin @('github.com', 'release-assets.githubusercontent.com', 'objects.githubusercontent.com')) {
            throw 'Download destination must be an ordinary approved HTTPS release URL.'
        }
        $request = [Net.HttpWebRequest]::Create($uri)
        $request.AllowAutoRedirect = $false
        $request.Timeout = 30000; $request.ReadWriteTimeout = 30000
        $request.UserAgent = 'Ilium-Installer/1'; $request.UseDefaultCredentials = $false
        $response = $null; $source = $null; $destinationStream = $null
        try {
            $response = $request.GetResponse()
            $code = [int]$response.StatusCode
            if ($code -in @(301, 302, 303, 307, 308)) {
                if (-not $response.Headers['Location'] -or $redirect -eq 5) { throw 'Invalid or excessive HTTPS redirects.' }
                $uri = New-Object Uri($uri, $response.Headers['Location'])
                continue
            }
            if ($code -ne 200) { throw "Unexpected HTTP status $code" }
            $bound = 1073741824
            if ($Url.EndsWith('/SHA256SUMS')) { $bound = 16384 }
            if ($response.ContentLength -gt $bound) { throw 'Download exceeds size bound.' }
            $source = $response.GetResponseStream()
            $destinationStream = [IO.File]::Open($Destination, 'CreateNew', 'Write', 'None')
            $buffer = New-Object byte[] 65536
            [long]$total = 0; $clock = [Diagnostics.Stopwatch]::StartNew()
            while (($count = $source.Read($buffer, 0, $buffer.Length)) -gt 0) {
                $total += $count
                if ($total -gt $bound -or $clock.Elapsed.TotalSeconds -gt 300) { throw 'Download exceeds time or size bound.' }
                $destinationStream.Write($buffer, 0, $count)
            }
            if ($total -eq 0) { throw 'Empty release asset.' }
            return
        } finally {
            if ($destinationStream) { $destinationStream.Dispose() }
            if ($source) { $source.Dispose() }
            if ($response) { $response.Dispose() }
        }
    }
    throw 'Download failed; requested release never falls back to latest.'
}

function Get-IliumLatestVersion {
    # Resolve once, then fetch both assets from that exact tag. Repeating latest
    # for each asset could mix two releases during publication.
    $request = [Net.HttpWebRequest]::Create('https://github.com/arthurwolf/ilium/releases/latest')
    $request.AllowAutoRedirect = $false; $request.Timeout = 30000
    $request.UserAgent = 'Ilium-Installer/1'; $request.UseDefaultCredentials = $false
    $response = $request.GetResponse()
    try {
        $location = $response.Headers['Location']
        if ([int]$response.StatusCode -notin @(301, 302, 303, 307, 308) -or
            $location -cnotmatch '\Ahttps://github\.com/arthurwolf/ilium/releases/tag/v(.+)\z' -or
            -not (Test-IliumVersion $Matches[1])) { throw 'Cannot resolve a valid latest stable release tag.' }
        $resolved = $Matches[1]
        if ($resolved.Contains('-')) { throw 'Latest stable release cannot be a prerelease.' }
        return $resolved
    } finally { $response.Dispose() }
}

function Get-IliumChecksum([string]$Path, [hashtable]$Target) {
    $content = [IO.File]::ReadAllText($Path)
    if ($content.Length -gt 16384 -or -not $content.EndsWith("`n")) { throw 'Malformed checksum manifest.' }
    $seen = New-Object 'Collections.Generic.HashSet[string]' ([StringComparer]::Ordinal)
    $selected = $null
    foreach ($line in $content.TrimEnd("`n").Split("`n")) {
        if ($line -cnotmatch '\A([0-9a-f]{64})  ([A-Za-z0-9._-]+)\z') { throw 'Malformed checksum record.' }
        $hash = $Matches[1]; $name = $Matches[2]
        if ($name -cnotin $Target.checksum_archives -or -not $seen.Add($name)) { throw 'Unexpected or duplicate checksum archive.' }
        if ($name -ceq $Target.archive) { $selected = $hash }
    }
    if ($seen.Count -ne $Target.checksum_archives.Count -or -not $selected) { throw 'Missing release checksum records.' }
    return $selected
}

function Expand-IliumValidatedZip([string]$Archive, [string]$Destination, [string]$Prefix, [string]$Version) {
    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($Archive)
    try {
        $seen = New-Object 'Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
        [long]$total = 0
        if ($zip.Entries.Count -gt 128) { throw 'Unexpected ZIP member count.' }
        foreach ($entry in $zip.Entries) {
            $name = $entry.FullName
            $attributes = [BitConverter]::ToUInt32([BitConverter]::GetBytes([int]$entry.ExternalAttributes), 0)
            $kind = ($attributes -shr 16) -band 61440
            if ($name -ceq ($Prefix + '/')) {
                if (-not $seen.Add('/') -or $entry.Length -ne 0 -or ($kind -ne 0 -and $kind -ne 16384) -or ($attributes -band 1024)) { throw 'Invalid ZIP root directory.' }
                continue
            }
            if (-not $name.StartsWith($Prefix + '/', [StringComparison]::Ordinal)) { throw 'Absolute or traversal ZIP path.' }
            $basename = $name.Substring($Prefix.Length + 1)
            if ($basename -cnotmatch '\A(?:ilium\.exe|ilium-server\.exe|VERSION|THIRD-PARTY\.txt|[A-Za-z0-9_-]+\.dll)\z' -or -not $seen.Add($basename)) {
                throw 'Unsafe traversal, duplicate or unexpected ZIP member.'
            }
            if (($kind -ne 0 -and $kind -ne 32768) -or ($attributes -band 1040)) { throw 'ZIP symlink, reparse point or non-regular file.' }
            $total += $entry.Length
            if ($entry.Length -le 0 -or $entry.Length -gt 1073741824 -or $total -gt 2000000000) { throw 'ZIP expanded size exceeds bound.' }
        }
        foreach ($required in @('ilium.exe', 'ilium-server.exe', 'VERSION', 'THIRD-PARTY.txt')) {
            if (-not $seen.Contains($required)) { throw 'ZIP is missing a required matched-pair member.' }
        }
        [IO.Directory]::CreateDirectory($Destination) | Out-Null
        foreach ($entry in $zip.Entries) {
            if ($entry.FullName -ceq ($Prefix + '/')) { continue }
            $basename = $entry.FullName.Substring($Prefix.Length + 1)
            $source = $entry.Open(); $output = $null
            try {
                $output = [IO.File]::Open((Join-Path $Destination $basename), 'CreateNew', 'Write', 'None')
                $buffer = New-Object byte[] 65536; [long]$written = 0
                while (($count = $source.Read($buffer, 0, $buffer.Length)) -gt 0) {
                    $written += $count
                    if ($written -gt $entry.Length) { throw 'ZIP entry exceeds declared length.' }
                    $output.Write($buffer, 0, $count)
                }
                if ($written -ne $entry.Length) { throw 'Truncated ZIP member.' }
            } finally { if ($output) { $output.Dispose() }; $source.Dispose() }
        }
    } finally { $zip.Dispose() }
    $versionPath = Join-Path $Destination 'VERSION'
    $expected = [Text.Encoding]::ASCII.GetBytes("$Version`n")
    $actual = [IO.File]::ReadAllBytes($versionPath)
    if ([Convert]::ToBase64String($actual) -cne [Convert]::ToBase64String($expected)) { throw 'Archive VERSION differs from the requested release.' }
}

function Confirm-IliumBinaryVersion([string]$Directory, [string]$Version) {
    foreach ($executable in @('ilium', 'ilium-server')) {
        $output = & (Join-Path $Directory ($executable + '.exe')) --version 2>&1
        if ($LASTEXITCODE -ne 0 -or ($output -join "`n").Trim() -cne "$executable $Version") { throw "Packaged $executable version identity failed." }
    }
}

function Get-IliumSigningStatus([string]$Directory) {
    $unsigned = $false
    foreach ($executable in @('ilium.exe', 'ilium-server.exe')) {
        $signature = Get-AuthenticodeSignature -LiteralPath (Join-Path $Directory $executable) -ErrorAction Stop
        if ($signature.Status -eq 'NotSigned') { $unsigned = $true }
        elseif ($signature.Status -ne 'Valid') { throw "Authenticode verification failed for $executable ($($signature.Status))." }
    }
    if ($unsigned) { return 'unsigned' }
    return 'verified'
}

function Get-IliumLauncher([string]$Root, [string]$Executable) {
    # ASCII batch source plus a base64 UTF-16 root avoids cmd code-page loss for
    # Unicode profile names. No delayed expansion; current is read only once.
    $encodedRoot = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($Root))
    $command = '$ErrorActionPreference=''Stop''; try { '
    $command += '$root=[Text.Encoding]::Unicode.GetString([Convert]::FromBase64String(''' + $encodedRoot + ''')); '
    $command += '$version=[IO.File]::ReadAllText((Join-Path $root ''current'')); '
    $command += 'if ($version -cnotmatch ''\A[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?\n\z'') { throw ''Invalid installed version pointer'' }; '
    $command += '$directory=Join-Path $root (''versions\''+$version.TrimEnd([char]10)+''\bin''); '
    $command += 'foreach ($item in @($root,(Join-Path $root ''current''),(Join-Path $root ''versions''),[IO.Directory]::GetParent($directory).FullName,$directory,(Join-Path $directory ''ilium.exe''),(Join-Path $directory ''ilium-server.exe''))) { '
    $command += 'if (-not (Test-Path -LiteralPath $item) -or (([IO.File]::GetAttributes($item) -band [IO.FileAttributes]::ReparsePoint) -ne 0)) { throw ''Installed binary pair is incomplete or unsafe'' } }; '
    $command += '& (Join-Path $directory ''' + $Executable + '.exe'') @args; exit $LASTEXITCODE '
    $command += '} catch { [Console]::Error.WriteLine($_.Exception.Message); exit 1 }'
    return '@echo off' + "`r`n" + 'setlocal DisableDelayedExpansion' + "`r`n" +
        '"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -Command "& { ' + $command + ' }" %*' + "`r`n" + 'exit /b %errorlevel%' + "`r`n"
}

function Get-IliumReceipt([string]$Directory) {
    $receipt = @{}
    foreach ($file in Get-ChildItem -LiteralPath $Directory -Force) {
        if ($file.PSIsContainer) { throw 'Unexpected directory in installed pair.' }
        $receipt[$file.Name] = Get-IliumHash $file.FullName
    }
    return $receipt
}
function Test-IliumOwnedVersion([string]$Root, [string]$Version, [switch]$AllowMissing, [string]$VersionsDirectory = '') {
    if (-not (Test-IliumVersion $Version)) { return $false }
    $receiptPath = Join-Path $Root ('installer-state/version-' + $Version + '.json')
    if (-not $VersionsDirectory) { $VersionsDirectory = Join-Path $Root 'versions' }
    $versionDirectory = Join-Path $VersionsDirectory $Version
    $directory = Join-Path $versionDirectory 'bin'
    Assert-IliumPlainPath $receiptPath; Assert-IliumPlainPath $directory
    if (-not [IO.File]::Exists($receiptPath) -or -not [IO.Directory]::Exists($directory)) { return $false }
    $receipt = [IO.File]::ReadAllText($receiptPath) | ConvertFrom-Json
    $names = @($receipt.PSObject.Properties.Name)
    foreach ($required in @('ilium.exe', 'ilium-server.exe', 'VERSION', 'THIRD-PARTY.txt')) { if ($required -cnotin $names) { return $false } }
    $children = @(Get-ChildItem -LiteralPath $versionDirectory -Force)
    if ($children.Count -ne 1 -or $children[0].Name -cne 'bin' -or -not $children[0].PSIsContainer) { return $false }
    foreach ($file in Get-ChildItem -LiteralPath $directory -Force) { if ($file.PSIsContainer -or $file.Name -cnotin $names) { return $false } }
    foreach ($property in $receipt.PSObject.Properties) {
        if ($property.Name -cnotmatch '\A(?:ilium\.exe|ilium-server\.exe|VERSION|THIRD-PARTY\.txt|[A-Za-z0-9_-]+\.dll)\z' -or $property.Value -cnotmatch '\A[0-9a-f]{64}\z') { return $false }
        $path = Join-Path $directory $property.Name
        Assert-IliumPlainPath $path
        if (-not [IO.File]::Exists($path)) { if ($AllowMissing) { continue }; return $false }
        if ((Get-IliumHash $path) -cne $property.Value) { return $false }
    }
    return $true
}
function Assert-IliumVersionUnlocked([string]$Directory, $Receipt) {
    $handles = @()
    try {
        foreach ($property in $Receipt.PSObject.Properties) {
            $path = Join-Path $Directory $property.Name
            if ([IO.File]::Exists($path)) { $handles += [IO.File]::Open($path, 'Open', 'ReadWrite', 'None') }
        }
    } finally { foreach ($handle in $handles) { $handle.Dispose() } }
}
function Stage-IliumOwnedVersion([string]$Root, [string]$Version, [string]$Destination) {
    if (-not (Test-IliumOwnedVersion $Root $Version)) { throw 'Version changed before uninstall staging; preserved.' }
    $receipt = [IO.File]::ReadAllText((Join-Path $Root ('installer-state/version-' + $Version + '.json'))) | ConvertFrom-Json
    $directory = Join-Path $Root ('versions/' + $Version)
    Assert-IliumVersionUnlocked (Join-Path $directory 'bin') $receipt
    Assert-IliumPlainPath $Destination
    # Move complete pairs, never delete individual members before uninstall's
    # commit. A failed rename/AV check can restore every prior pair intact.
    [IO.Directory]::Move($directory, $Destination)
}
function Remove-IliumOwnedVersion([string]$Root, [string]$Version, [string]$VersionsDirectory = '', [switch]$AllowMissing) {
    if (-not (Test-IliumOwnedVersion $Root $Version -AllowMissing:$AllowMissing -VersionsDirectory $VersionsDirectory)) { Write-Warning "Preserved modified or unknown version $Version"; return }
    $receiptPath = Join-Path $Root ('installer-state/version-' + $Version + '.json')
    $receipt = [IO.File]::ReadAllText($receiptPath) | ConvertFrom-Json
    if (-not $VersionsDirectory) { $VersionsDirectory = Join-Path $Root 'versions' }
    $directory = Join-Path (Join-Path $VersionsDirectory $Version) 'bin'
    # Detect locked binaries before deleting any member, so running old
    # sessions do not leave a partially pruned version.
    Assert-IliumVersionUnlocked $directory $receipt
    foreach ($property in $receipt.PSObject.Properties) { [IO.File]::Delete((Join-Path $directory $property.Name)) }
    [IO.Directory]::Delete($directory, $false)
    [IO.Directory]::Delete([IO.Directory]::GetParent($directory).FullName, $false)
    [IO.File]::Delete($receiptPath)
}

function Recover-IliumTransaction([string]$Root, [string]$Bin, [switch]$PathApplied, [string[]]$PublishedLaunchers = @()) {
    $journalPath = Join-Path $Root 'installer-state/transaction.json'
    if (-not [IO.File]::Exists($journalPath)) { return }
    Assert-IliumPlainPath $journalPath
    $journal = [IO.File]::ReadAllText($journalPath) | ConvertFrom-Json
    if ($journal.schema -ne 1 -or $journal.operation -cnotin @('install', 'uninstall') -or $journal.bin -cne $Bin -or
        $journal.ready -isnot [bool] -or $journal.path_applied -isnot [bool] -or $journal.path_added -isnot [bool]) { throw 'Invalid transaction recovery journal; preserved.' }
    $oldState = $journal.old_state | ConvertFrom-Json
    if ($oldState.schema -ne 1 -or $oldState.owner -cne 'ilium-windows-installer-1' -or $oldState.root -cne $Root -or $oldState.bin -cne $Bin -or $oldState.path_added -isnot [bool]) { throw 'Invalid transaction ownership snapshot; preserved.' }
    $pointerPath = Join-Path $Root 'current'
    Assert-IliumPlainPath $pointerPath
    if ($journal.operation -ceq 'uninstall') {
        if ($journal.quarantine -cnotmatch '\Auninstall-[0-9a-f]{32}\z' -or $journal.old_pointer -isnot [string] -or
            ($journal.old_pointer -and $journal.old_pointer -cnotmatch '\A[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?\n\z')) { throw 'Invalid uninstall recovery journal; preserved.' }
        $seen = New-Object 'Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
        foreach ($version in @($journal.versions)) { if (-not (Test-IliumVersion $version) -or -not $seen.Add($version)) { throw 'Invalid uninstall version inventory; preserved.' } }
        $seen.Clear()
        foreach ($executable in @($journal.launchers)) { if ($executable -cnotin @('ilium', 'ilium-server') -or -not $seen.Add($executable)) { throw 'Invalid uninstall launcher inventory; preserved.' } }
        $quarantine = Join-Path (Join-Path $Root 'installer-state') $journal.quarantine
        $stagedVersions = Join-Path $quarantine 'versions'
        $stagedLaunchers = Join-Path $quarantine 'launchers'
        Assert-IliumPlainPath $quarantine
        foreach ($version in @($journal.versions)) {
            $staged = Join-Path $stagedVersions $version
            Assert-IliumPlainPath $staged
            if (-not [IO.Directory]::Exists($staged)) { continue }
            if ($journal.ready) { Remove-IliumOwnedVersion $Root $version -VersionsDirectory $stagedVersions -AllowMissing; continue }
            if (-not (Test-IliumOwnedVersion $Root $version -VersionsDirectory $stagedVersions)) { throw 'Modified uninstall staging; preserved.' }
            $original = Join-Path (Join-Path $Root 'versions') $version
            Assert-IliumPlainPath $original
            if (Test-Path -LiteralPath $original) { throw 'Uninstall rollback destination changed; preserved.' }
            [IO.Directory]::Move($staged, $original)
        }
        foreach ($executable in @($journal.launchers)) {
            $staged = Join-Path $stagedLaunchers ($executable + '.cmd')
            Assert-IliumPlainPath $staged
            if (-not [IO.File]::Exists($staged)) { continue }
            if ([IO.File]::ReadAllText($staged) -cne (Get-IliumLauncher $Root $executable)) { throw 'Modified staged launcher; preserved.' }
            if ($journal.ready) { [IO.File]::Delete($staged); continue }
            $original = Join-Path $Bin ($executable + '.cmd')
            Assert-IliumPlainPath $original
            [IO.File]::Move($staged, $original)
        }
        if (-not $journal.ready) {
            if ($journal.old_pointer -and -not (Test-Path -LiteralPath $pointerPath)) { Write-IliumAtomic $pointerPath $journal.old_pointer -NoReplace }
            if ($journal.old_pointer -and [IO.File]::ReadAllText($pointerPath) -cne $journal.old_pointer) { throw 'Uninstall pointer changed; preserved.' }
            Write-IliumAtomic (Join-Path $Root 'installer-state/state.json') $journal.old_state
            Restore-IliumTransactionPath $journal -PathApplied:$PathApplied
        }
        # Non-recursive cleanup deliberately fails if unknown/modified content
        # appeared. Keep the journal for retry rather than overwriting it.
        foreach ($directory in @($stagedVersions, $stagedLaunchers, $quarantine)) { if ([IO.Directory]::Exists($directory)) { [IO.Directory]::Delete($directory, $false) } }
        [IO.File]::Delete($journalPath)
        return
    }
    if (-not (Test-IliumVersion $journal.next)) { throw 'Invalid install transaction version; preserved.' }
    $activated = $journal.ready -and [IO.File]::Exists($pointerPath) -and [IO.File]::ReadAllText($pointerPath) -ceq ($journal.next + "`n")
    if (-not $activated) {
        Restore-IliumTransactionPath $journal -PathApplied:$PathApplied
        foreach ($executable in (@($journal.new_launchers) + @($PublishedLaunchers))) {
            if ($executable -cnotin @('ilium', 'ilium-server')) { throw 'Invalid journal launcher identity.' }
            $launcher = Join-Path $Bin ($executable + '.cmd')
            Assert-IliumPlainPath $launcher
            if ([IO.File]::Exists($launcher) -and [IO.File]::ReadAllText($launcher) -ceq (Get-IliumLauncher $Root $executable)) { [IO.File]::Delete($launcher) }
        }
        Write-IliumAtomic (Join-Path $Root 'installer-state/state.json') $journal.old_state
    }
    [IO.File]::Delete($journalPath)
}

function Restore-IliumTransactionPath($Journal, [switch]$PathApplied) {
    # Intent alone is not ownership. A crash between registry write and journal
    # acknowledgement leaves an ambiguous entry preserved, never claimed.
    if (-not $Journal.path_applied -and -not $PathApplied) { return }
    $current = Get-IliumUserPath
    if ($current -ceq $Journal.path_after) { Set-IliumUserPath $Journal.path_before; return }
    # Only uninstall can prove a missing entry was ours: its acknowledged
    # before/after snapshots must differ by removal of the exact owned bin.
    # Restore that one entry by append, retaining every concurrent byte/order.
    # Install rollback remains conservative: never delete an ambiguous entry.
    if ($Journal.operation -ceq 'uninstall' -and $Journal.path_added -and
        $Journal.path_before -cne $Journal.path_after -and
        (Remove-IliumPathEntry $Journal.path_before $Journal.bin) -ceq $Journal.path_after) {
        if (@($current -split ';' | Where-Object { Test-IliumPathEntry $_ $Journal.bin }).Count) { return }
        if ((Get-IliumUserPath) -cne $current) { throw 'User PATH changed again during uninstall recovery; retry safely.' }
        $restored = $Journal.bin
        if ($null -ne $current) { $restored = $current + ';' + $Journal.bin }
        Set-IliumUserPath $restored
        return
    }
    Write-Warning 'Concurrent user PATH edits were preserved; ambiguous PATH ownership was not assumed.'
}

function Get-IliumRecoveryCommand {
    param([string]$Version, [string]$InstallDir, [string]$BinDir, [switch]$NoModifyPath, [switch]$Uninstall)
    # Recovery also works after an HTTPS bootstrap without a local script.
    # Single-quoted PowerShell arguments treat even rejected input as data.
    $command = "& ([scriptblock]::Create((Invoke-RestMethod -Uri 'https://ilium-setup.pages.dev/install.ps1'))) -Version '" + $Version.Replace("'", "''") +
        "' -InstallDir '" + $InstallDir.Replace("'", "''") + "' -BinDir '" + $BinDir.Replace("'", "''") + "'"
    if ($NoModifyPath) { $command += ' -NoModifyPath' }
    if ($Uninstall) { $command += ' -Uninstall' }
    return $command
}

function Invoke-IliumInstall {
    [CmdletBinding()]
    param([string]$Version = 'latest', [string]$InstallDir = '', [string]$BinDir = '', [switch]$NoModifyPath, [switch]$Uninstall)
    $ErrorActionPreference = 'Stop'
    Set-StrictMode -Version 2.0
    $stage = 'arguments'; $target = 'unknown'; $priorActive = 'no'
    $lock = $null; $work = $null; $root = $null; $bin = $null; $committed = $false
    $pathApplied = $false; $publishedLaunchers = @()
    $recoveryArguments = @{Version=$Version; InstallDir=$InstallDir; BinDir=$BinDir; NoModifyPath=[bool]$NoModifyPath; Uninstall=[bool]$Uninstall}
    $oldTls = [Net.ServicePointManager]::SecurityProtocol
    try {
        if ($PSVersionTable.PSVersion -lt [Version]'5.1') { throw 'PowerShell 5.1 or newer is required.' }
        if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'Windows is required.' }
        if ($Version -cne 'latest') { $Version = $Version -creplace '\Av', ''; if (-not (Test-IliumVersion $Version)) { throw 'Invalid semantic release version.' } }
        $selected = Get-IliumTarget (Get-IliumNativeArchitecture)
        $target = $selected.target
        if (-not $InstallDir) { $InstallDir = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'ilium' }
        $root = Get-IliumSafePath $InstallDir
        if (-not $BinDir) { $BinDir = Join-Path $root 'bin' }
        $bin = Get-IliumSafePath $BinDir
        if ($bin -ieq $root -or $bin.StartsWith($root + '\versions', [StringComparison]::OrdinalIgnoreCase) -or $bin.StartsWith($root + '\installer-state', [StringComparison]::OrdinalIgnoreCase)) { throw 'BinDir overlaps installation metadata or versions.' }
        $stateDirectory = Join-Path $root 'installer-state'
        $statePath = Join-Path $stateDirectory 'state.json'
        $pointerPath = Join-Path $root 'current'
        $stage = 'ownership'
        Assert-IliumPlainPath $statePath; Assert-IliumPlainPath $pointerPath
        if ([IO.Directory]::Exists($root) -and -not [IO.File]::Exists($statePath)) {
            $unknown = @(Get-ChildItem -LiteralPath $root -Force | Where-Object { $_.Name -cne '.install-lock' })
            if ($unknown.Count) { throw 'Refusing a nonempty unowned install directory.' }
        }
        if ($Uninstall -and -not [IO.File]::Exists($statePath)) { Write-Output 'ilium-install: stage=complete action=uninstall owned_installation=absent'; return }
        New-IliumPrivateDirectory $root
        $stage = 'lock'
        $lockPath = Join-Path $root '.install-lock'; Assert-IliumPlainPath $lockPath
        try { $lock = [IO.File]::Open($lockPath, 'OpenOrCreate', 'ReadWrite', 'None') }
        catch { throw 'Installation is locked by another installer or filesystem access was denied.' }
        # All mutable state is read under the same exclusive installation lock.
        $stage = 'ownership'
        Assert-IliumPlainPath $stateDirectory
        [IO.Directory]::CreateDirectory($stateDirectory) | Out-Null
        if (-not [IO.File]::Exists($statePath)) {
            $initial = @{ schema=1; owner='ilium-windows-installer-1'; root=$root; bin=$bin; path_added=$false; previous='' }
            Write-IliumAtomic $statePath ($initial | ConvertTo-Json -Compress)
        }
        $state = [IO.File]::ReadAllText($statePath) | ConvertFrom-Json
        if ($state.schema -ne 1 -or $state.owner -cne 'ilium-windows-installer-1' -or $state.root -cne $root -or $state.bin -cne $bin -or $state.path_added -isnot [bool] -or ($state.previous -and -not (Test-IliumVersion $state.previous))) { throw 'Invalid installer ownership state or a changed BinDir; preserved.' }
        Recover-IliumTransaction $root $bin
        if ([IO.File]::Exists((Join-Path $stateDirectory 'transaction.json'))) { throw 'Transaction recovery is incomplete; retry before starting another operation.' }
        $oldState = [IO.File]::ReadAllText($statePath)
        $state = $oldState | ConvertFrom-Json
        $previous = ''
        if ([IO.File]::Exists($pointerPath)) {
            $pointer = [IO.File]::ReadAllText($pointerPath)
            if (-not $pointer.EndsWith("`n") -or -not (Test-IliumVersion $pointer.TrimEnd([char]10)) -or $pointer -cne ($pointer.TrimEnd([char]10) + "`n")) { throw 'Invalid current version pointer; preserved.' }
            $previous = $pointer.TrimEnd([char]10)
            if (-not [IO.File]::Exists((Join-Path $stateDirectory ('version-' + $previous + '.json')))) { throw 'Current pointer is not installer-owned.' }
            $priorActive = 'yes'
        }
        if ($Uninstall) {
            $stage = 'uninstall'
            $ownedVersions = @(); $ownedLaunchers = @()
            foreach ($receipt in Get-ChildItem -LiteralPath $stateDirectory -Filter 'version-*.json' -Force) {
                $ownedVersion = $receipt.Name.Substring(8, $receipt.Name.Length - 13)
                if ((Test-IliumVersion $ownedVersion) -and (Test-IliumOwnedVersion $root $ownedVersion)) { $ownedVersions += $ownedVersion }
                else { Write-Warning "Preserved modified or unknown version $ownedVersion" }
            }
            foreach ($executable in @('ilium', 'ilium-server')) {
                $launcher = Join-Path $bin ($executable + '.cmd'); Assert-IliumPlainPath $launcher
                if ([IO.File]::Exists($launcher) -and [IO.File]::ReadAllText($launcher) -ceq (Get-IliumLauncher $root $executable)) { $ownedLaunchers += $executable }
            }
            $userPath = Get-IliumUserPath
            $updatedPath = $userPath
            if ($state.path_added) { $updatedPath = Remove-IliumPathEntry $userPath $bin }
            $oldPointer = ''; if ($previous) { $oldPointer = $previous + "`n" }
            $journal = @{schema=1; operation='uninstall'; ready=$false; bin=$bin; old_state=$oldState; old_pointer=$oldPointer;
                quarantine=('uninstall-' + [guid]::NewGuid().ToString('N')); versions=@($ownedVersions); launchers=@($ownedLaunchers);
                path_added=[bool]$state.path_added; path_applied=$false; path_before=$userPath; path_after=$updatedPath}
            Write-IliumAtomic (Join-Path $stateDirectory 'transaction.json') ($journal | ConvertTo-Json -Compress)
            $quarantine = Join-Path $stateDirectory $journal.quarantine
            [IO.Directory]::CreateDirectory((Join-Path $quarantine 'versions')) | Out-Null
            [IO.Directory]::CreateDirectory((Join-Path $quarantine 'launchers')) | Out-Null
            foreach ($ownedVersion in $ownedVersions) { Stage-IliumOwnedVersion $root $ownedVersion (Join-Path $quarantine ('versions/' + $ownedVersion)) }
            foreach ($executable in $ownedLaunchers) {
                $launcher = Join-Path $bin ($executable + '.cmd')
                Assert-IliumPlainPath $launcher
                if ([IO.File]::ReadAllText($launcher) -cne (Get-IliumLauncher $root $executable)) { throw 'Launcher changed before uninstall staging; preserved.' }
                [IO.File]::Move($launcher, (Join-Path $quarantine ('launchers/' + $executable + '.cmd')))
            }
            if ($updatedPath -cne $userPath) {
                if ((Get-IliumUserPath) -cne $userPath) { throw 'User PATH changed concurrently; uninstall rolled back.' }
                Set-IliumUserPath $updatedPath
                $pathApplied = $true
                if ((Get-IliumUserPath) -cne $updatedPath) { throw 'User PATH write readback differs; concurrent edits preserved.' }
                $journal.path_applied = $true
                Write-IliumAtomic (Join-Path $stateDirectory 'transaction.json') ($journal | ConvertTo-Json -Compress)
            }
            if ([IO.File]::Exists($pointerPath)) { [IO.File]::Delete($pointerPath) }
            $state.path_added = $false
            Write-IliumAtomic $statePath ($state | ConvertTo-Json -Compress)
            $journal.ready = $true
            Write-IliumAtomic (Join-Path $stateDirectory 'transaction.json') ($journal | ConvertTo-Json -Compress)
            $committed = $true
            try { Recover-IliumTransaction $root $bin }
            catch { Write-Warning "Uninstall cleanup deferred at $quarantine; retry this installer with -Uninstall." }
            Write-Output 'ilium-install: stage=complete action=uninstall unknown_content=preserved'
            return
        }
        foreach ($executable in @('ilium', 'ilium-server')) {
            $launcher = Join-Path $bin ($executable + '.cmd'); Assert-IliumPlainPath $launcher
            if ([IO.Directory]::Exists($launcher) -or ([IO.File]::Exists($launcher) -and [IO.File]::ReadAllText($launcher) -cne (Get-IliumLauncher $root $executable))) { throw 'Refusing an unowned or modified launcher.' }
        }
        $stage = 'staging'
        $versions = Join-Path $root 'versions'; Assert-IliumPlainPath $versions
        [IO.Directory]::CreateDirectory($versions) | Out-Null
        New-IliumPrivateDirectory $bin
        if ((Get-IliumFreeSpace $root) -lt 67108864) { throw 'At least 64 MiB of free space is required.' }
        $work = Join-Path ([IO.Path]::GetTempPath()) ('ilium-install-' + [guid]::NewGuid().ToString('N'))
        New-IliumPrivateDirectory $work
        $stage = 'download'
        $secureTls = [Net.SecurityProtocolType]::Tls12
        if ([Enum]::GetNames([Net.SecurityProtocolType]) -contains 'Tls13') { $secureTls = $secureTls -bor [Net.SecurityProtocolType]::Tls13 }
        [Net.ServicePointManager]::SecurityProtocol = $secureTls
        if ($Version -ceq 'latest') { $Version = Get-IliumLatestVersion }
        $origin = 'https://github.com/arthurwolf/ilium/releases/download/v' + $Version
        $checksums = Join-Path $work 'SHA256SUMS'; $archive = Join-Path $work 'archive.zip'
        Save-IliumDownload ($origin + '/SHA256SUMS') $checksums
        $expectedHash = Get-IliumChecksum $checksums $selected
        Save-IliumDownload ($origin + '/' + $selected.archive) $archive
        $stage = 'checksum'
        if ((Get-IliumHash $archive) -cne $expectedHash) { throw 'Archive checksum mismatch; previous installation remains selected.' }
        $stage = 'archive'
        $payload = Join-Path $work 'payload'
        Expand-IliumValidatedZip $archive $payload $selected.prefix $Version
        Confirm-IliumBinaryVersion $payload $Version
        $signing = Get-IliumSigningStatus $payload
        $receipt = Get-IliumReceipt $payload
        $final = Join-Path $versions $Version
        $receiptPath = Join-Path $stateDirectory ('version-' + $Version + '.json')
        Assert-IliumPlainPath $final; Assert-IliumPlainPath $receiptPath
        $stage = 'version-install'
        if ([IO.Directory]::Exists($final)) {
            if (-not (Test-IliumOwnedVersion $root $Version -AllowMissing)) { throw 'Existing immutable version is modified or unowned; preserved.' }
            $oldReceipt = [IO.File]::ReadAllText($receiptPath) | ConvertFrom-Json
            if (@($oldReceipt.PSObject.Properties).Count -ne $receipt.Count) { throw 'Existing immutable archive inventory differs.' }
            foreach ($name in $receipt.Keys) {
                if ($oldReceipt.$name -cne $receipt[$name]) { throw 'Existing immutable version differs from verified archive.' }
                $path = Join-Path $final ('bin/' + $name)
                if (-not [IO.File]::Exists($path)) {
                    $repair = Join-Path $root ('.repair-' + [guid]::NewGuid().ToString('N'))
                    try {
                        [IO.File]::Copy((Join-Path $payload $name), $repair, $false)
                        if ((Get-IliumHash $repair) -cne $receipt[$name]) { throw 'Repair checksum mismatch.' }
                        Assert-IliumPlainPath $path
                        [IO.File]::Move($repair, $path)
                    } finally { if ([IO.File]::Exists($repair)) { [IO.File]::Delete($repair) } }
                }
            }
        } else {
            if ([IO.File]::Exists($final)) { throw 'Version destination is not a directory.' }
            # Publish receipt first: interruption cannot turn a complete newly
            # moved directory into unowned content on the next retry.
            if ([IO.File]::Exists($receiptPath)) {
                $saved = [IO.File]::ReadAllText($receiptPath) | ConvertFrom-Json
                if (@($saved.PSObject.Properties).Count -ne $receipt.Count) { throw 'Retained version receipt differs.' }
                foreach ($name in $receipt.Keys) { if ($saved.$name -cne $receipt[$name]) { throw 'Retained immutable receipt differs.' } }
            } else { Write-IliumAtomic $receiptPath ($receipt | ConvertTo-Json -Compress) }
            # Cross-volume temporary staging is copied to a root-local candidate
            # and verified before atomic directory publication on this volume.
            $candidate = Join-Path $root ('.candidate-' + [guid]::NewGuid().ToString('N'))
            [IO.Directory]::CreateDirectory((Join-Path $candidate 'bin')) | Out-Null
            try {
                foreach ($name in $receipt.Keys) {
                    $destination = Join-Path $candidate ('bin/' + $name)
                    [IO.File]::Copy((Join-Path $payload $name), $destination, $false)
                    if ((Get-IliumHash $destination) -cne $receipt[$name]) { throw 'Version copy checksum mismatch.' }
                }
                [IO.Directory]::Move($candidate, $final)
            } finally {
                if ([IO.Directory]::Exists($candidate)) {
                    foreach ($name in $receipt.Keys) { $owned = Join-Path $candidate ('bin/' + $name); if ([IO.File]::Exists($owned)) { [IO.File]::Delete($owned) } }
                    [IO.Directory]::Delete((Join-Path $candidate 'bin'), $false); [IO.Directory]::Delete($candidate, $false)
                }
            }
        }
        $stage = 'launchers'
        $newLaunchers = @()
        foreach ($executable in @('ilium', 'ilium-server')) { if (-not [IO.File]::Exists((Join-Path $bin ($executable + '.cmd')))) { $newLaunchers += $executable } }
        $userPath = Get-IliumUserPath
        $addPath = -not $NoModifyPath -and -not (@($userPath -split ';' | Where-Object { Test-IliumPathEntry $_ $bin }).Count)
        $updatedPath = $userPath
        if ($addPath) { $updatedPath = $bin; if ($userPath) { $updatedPath = $userPath + ';' + $bin } }
        if ($addPath -and $updatedPath.Length -gt 8192) { throw 'User PATH exceeds the conservative installer length bound.' }
        $journal = @{ schema=1; operation='install'; ready=$false; next=$Version; bin=$bin; path_added=[bool]$addPath; path_applied=$false; path_before=$userPath; path_after=$updatedPath; new_launchers=@(); old_state=$oldState }
        Write-IliumAtomic (Join-Path $stateDirectory 'transaction.json') ($journal | ConvertTo-Json -Compress)
        foreach ($executable in $newLaunchers) {
            $launcher = Join-Path $bin ($executable + '.cmd')
            # Never replace a launcher that appeared during staging.
            if (Test-Path -LiteralPath $launcher) { throw 'Launcher destination changed; preserved.' }
            $launcherStage = Join-Path $bin ('.ilium-launcher-' + [guid]::NewGuid().ToString('N'))
            try {
                $stream = [IO.File]::Open($launcherStage, 'CreateNew', 'Write', 'None')
                try {
                    $bytes = [Text.Encoding]::ASCII.GetBytes((Get-IliumLauncher $root $executable))
                    $stream.Write($bytes, 0, $bytes.Length); $stream.Flush($true)
                } finally { $stream.Dispose() }
                Assert-IliumPlainPath $launcher
                [IO.File]::Move($launcherStage, $launcher)
                $publishedLaunchers += $executable
                $journal.new_launchers = @($publishedLaunchers)
                Write-IliumAtomic (Join-Path $stateDirectory 'transaction.json') ($journal | ConvertTo-Json -Compress)
            } finally { if ([IO.File]::Exists($launcherStage)) { [IO.File]::Delete($launcherStage) } }
        }
        $stage = 'path'
        if ($addPath) {
            $pathNow = Get-IliumUserPath
            if ($pathNow -cne $userPath) { throw 'User PATH changed concurrently; retry without discarding other entries.' }
            Set-IliumUserPath $updatedPath
            $pathApplied = $true
            if ((Get-IliumUserPath) -cne $updatedPath) { throw 'User PATH write readback differs; concurrent edits preserved.' }
            $journal.path_applied = $true
            Write-IliumAtomic (Join-Path $stateDirectory 'transaction.json') ($journal | ConvertTo-Json -Compress)
            $state.path_added = $true
        }
        if ($previous -and $previous -cne $Version) { $state.previous = $previous }
        Write-IliumAtomic $statePath ($state | ConvertTo-Json -Compress)
        $stage = 'switch'
        if (-not (Test-IliumOwnedVersion $root $Version)) { throw 'New version pair is incomplete.' }
        foreach ($executable in @('ilium', 'ilium-server')) {
            if ([IO.File]::ReadAllText((Join-Path $bin ($executable + '.cmd'))) -cne (Get-IliumLauncher $root $executable)) { throw 'Launcher no longer resolves the owned binary pair.' }
        }
        $journal.ready = $true
        Write-IliumAtomic (Join-Path $stateDirectory 'transaction.json') ($journal | ConvertTo-Json -Compress)
        if ($previous -cne $Version) { Write-IliumAtomic $pointerPath ($Version + "`n") }
        $committed = $true
        # Maintenance cannot invalidate an already activated pair. Locked old
        # executables (running sessions) are preserved and never terminated.
        try {
            Recover-IliumTransaction $root $bin
            foreach ($old in Get-ChildItem -LiteralPath $stateDirectory -Filter 'version-*.json' -Force) {
                $oldVersion = $old.Name.Substring(8, $old.Name.Length - 13)
                if ((Test-IliumVersion $oldVersion) -and $oldVersion -cne $Version -and $oldVersion -cne $state.previous) { Remove-IliumOwnedVersion $root $oldVersion }
            }
        } catch { Write-Warning 'Old version or transaction maintenance was deferred; retry safely.' }
        Write-Output "ilium-install: stage=complete version=$Version target=$target matched_pair=verified"
        Write-Output "Authenticode: $signing. This installer does not bypass SmartScreen."
        if (-not (@($env:Path -split ';' | Where-Object { Test-IliumPathEntry $_ $bin }).Count)) {
            $quoted = $bin.Replace("'", "''")
            Write-Output ("For this PowerShell: `$env:Path += ';" + $quoted + "'")
        }
    } catch {
        $failure = $_
        if (-not $committed -and $lock -and $root -and $bin) {
            try { Recover-IliumTransaction $root $bin -PathApplied:$pathApplied -PublishedLaunchers $publishedLaunchers } catch { Write-Warning 'Recovery deferred; retry this installer to restore the prior installation.' }
        }
        [Console]::Error.WriteLine("ilium-install: stage=$stage version=$Version target=$target prior_active=$priorActive error=$($failure.Exception.Message)")
        [Console]::Error.WriteLine('ilium-install: recovery=' + (Get-IliumRecoveryCommand @recoveryArguments))
        throw $failure
    } finally {
        [Net.ServicePointManager]::SecurityProtocol = $oldTls
        if ($work -and [IO.Directory]::Exists($work)) {
            try { Assert-IliumPlainPath $work; [IO.Directory]::Delete($work, $true) }
            catch { Write-Warning "Owned temporary files retained at $work" }
        }
        if ($lock) { $lock.Dispose() }
    }
}

Invoke-IliumInstall -Version $Version -InstallDir $InstallDir -BinDir $BinDir -NoModifyPath:$NoModifyPath -Uninstall:$Uninstall
