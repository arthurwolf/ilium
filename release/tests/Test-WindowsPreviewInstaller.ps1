# Native, nonpublishing check of the checked-out installer against the public
# preview.6 release. Run only on a disposable GitHub-hosted Windows account.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$Installer,
      [Parameter(Mandatory)][string]$EvidenceDirectory)
$ErrorActionPreference = 'Stop'
function Get-IliumUserPathSnapshot {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
    if ($null -eq $key) { return @{ exists=$false; kind=$null; value=$null } }
    try {
        if ($key.GetValueNames() -notcontains 'Path') { return @{ exists=$false; kind=$null; value=$null } }
        $kind = $key.GetValueKind('Path')
        if ($kind -notin @([Microsoft.Win32.RegistryValueKind]::String, [Microsoft.Win32.RegistryValueKind]::ExpandString)) {
            throw 'User PATH has an unsupported registry value kind.'
        }
        $value = $key.GetValue('Path', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        if ($value -isnot [string]) { throw 'User PATH is not a registry string.' }
        return @{ exists=$true; kind=$kind.ToString(); value=$value }
    } finally { $key.Dispose() }
}

function Test-IliumSameUserPath($Left, $Right) {
    return $Left.exists -eq $Right.exists -and $Left.kind -ceq $Right.kind -and $Left.value -ceq $Right.value
}

function Restore-IliumUserPath($Snapshot) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
    if ($null -eq $key) {
        if (-not $Snapshot.exists) { return }
        $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
    }
    try {
        if ($Snapshot.exists) {
            $key.SetValue('Path', $Snapshot.value, [Microsoft.Win32.RegistryValueKind]$Snapshot.kind)
        } elseif ($key.GetValueNames() -contains 'Path') {
            $key.DeleteValue('Path', $false)
        }
    } finally { $key.Dispose() }
}

$expectedVersion = '0.1.1-preview.6'
$root = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'ilium'
$bin = Join-Path $root 'bin'
$state = Join-Path $root 'installer-state/state.json'
$errors = New-Object 'Collections.Generic.List[string]'
$receipt = @{ schema=1; state='failed'; installer_sha256=$null; version=$expectedVersion;
    client_version=$null; server_version=$null; client_sha256=$null; server_sha256=$null;
    cleanup='not-run'; path_restored_by_uninstall=$false; path_restored=$false; path_compensation_applied=$false }

try {
    if ($env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted' -or
        $env:GITHUB_REPOSITORY -cne 'arthurwolf/ilium' -or
        [Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT -or
        -not [Environment]::Is64BitOperatingSystem) {
        throw 'This native installer check requires a disposable GitHub-hosted Windows x86-64 account.'
    }
    $installerPath = [IO.Path]::GetFullPath($Installer)
    if (-not [IO.File]::Exists($installerPath)) { throw 'Checked-out candidate installer is missing.' }
    $evidencePath = [IO.Path]::GetFullPath($EvidenceDirectory)
    $runnerTemporary = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\') + '\'
    if (-not $evidencePath.StartsWith($runnerTemporary, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Evidence directory must be within the disposable runner temporary directory.'
    }
    if (Test-Path -LiteralPath $root) { throw 'Disposable account already has an Ilium installation; refusing to overwrite it.' }
    [IO.Directory]::CreateDirectory($evidencePath) | Out-Null
    $originalPath = Get-IliumUserPathSnapshot
    $receipt.path_before = $originalPath
    $receipt.installer_sha256 = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash.ToLowerInvariant()

    try {
        # Preview.7 is not public until native qualification finishes. Exercise
        # the existing published archive explicitly; Pages tests the default
        # one-liner and preview.7 after that release has been published.
        $installOutput = @(& $installerPath -Version $expectedVersion)
        if (($installOutput -join "`n") -notmatch 'stage=complete version=0\.1\.1-preview\.6') {
            throw 'Candidate installer did not report the pinned Windows preview.'
        }
        $pointer = Join-Path $root 'current'
        if ([IO.File]::ReadAllText($pointer) -cne ($expectedVersion + "`n")) { throw 'Installed version pointer differs.' }
        $pair = Join-Path $root ('versions/' + $expectedVersion + '/bin')
        foreach ($name in @('ilium.exe', 'ilium-server.exe', 'ilium-animation-helper.exe')) {
            if (-not [IO.File]::Exists((Join-Path $pair $name))) { throw "Installed pair is missing $name." }
        }
        foreach ($name in @('ilium.cmd', 'ilium-server.cmd')) {
            if (-not [IO.File]::Exists((Join-Path $bin $name))) { throw "Installer launcher is missing $name." }
        }
        $client = Join-Path $pair 'ilium.exe'
        $server = Join-Path $pair 'ilium-server.exe'
        $clientVersion = (& $client --version 2>&1 | Out-String).Trim()
        if ($LASTEXITCODE -ne 0 -or $clientVersion -cne ('ilium ' + $expectedVersion)) { throw 'Installed client version check failed.' }
        $serverVersion = (& $server --version 2>&1 | Out-String).Trim()
        if ($LASTEXITCODE -ne 0 -or $serverVersion -cne ('ilium-server ' + $expectedVersion)) { throw 'Installed server version check failed.' }
        $receipt.client_version = $clientVersion
        $receipt.server_version = $serverVersion
        $receipt.client_sha256 = (Get-FileHash -LiteralPath $client -Algorithm SHA256).Hash.ToLowerInvariant()
        $receipt.server_sha256 = (Get-FileHash -LiteralPath $server -Algorithm SHA256).Hash.ToLowerInvariant()
    } catch { $errors.Add('installation: ' + $_.Exception.Message) }
    finally {
        $uninstallSucceeded = $false
        try {
            if ([IO.File]::Exists($state)) {
                $null = & $installerPath -Uninstall
                $uninstallSucceeded = $true
            }
        } catch { $errors.Add('uninstall: ' + $_.Exception.Message) }
        try {
            $afterUninstall = Get-IliumUserPathSnapshot
            $receipt.path_after_uninstall = $afterUninstall
            $receipt.path_restored_by_uninstall = $uninstallSucceeded -and (Test-IliumSameUserPath $afterUninstall $originalPath)
            if (-not $uninstallSucceeded) {
                $errors.Add('uninstall: no successful owned uninstall; PATH restoration is unverified.')
            } elseif (-not $receipt.path_restored_by_uninstall) {
                $errors.Add('uninstall: User PATH bytes, registry kind or absence differ from the original before compensation.')
            }
        } catch { $errors.Add('uninstall PATH readback: ' + $_.Exception.Message) }
        try {
            if ([IO.Directory]::Exists($root)) {
                if (([IO.File]::GetAttributes($root) -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                    throw 'Installation root became a reparse point; refusing cleanup.'
                }
                $remaining = @(Get-ChildItem -LiteralPath $root -Force)
                if ($remaining.Count -eq 1 -and $remaining[0].Name -ceq '.install-lock' -and -not $remaining[0].PSIsContainer) {
                    [IO.File]::Delete($remaining[0].FullName)
                    $remaining = @()
                }
                if ($remaining.Count -ne 0) { throw 'Uninstall retained content; refusing broad cleanup.' }
                [IO.Directory]::Delete($root, $false)
            }
            $receipt.cleanup = 'owned-installation-removed'
        } catch { $errors.Add('cleanup: ' + $_.Exception.Message); $receipt.cleanup = 'incomplete' }
        try {
            $beforeCompensation = Get-IliumUserPathSnapshot
            if (-not (Test-IliumSameUserPath $beforeCompensation $originalPath)) {
                Restore-IliumUserPath $originalPath
                $receipt.path_compensation_applied = $true
            }
            $afterCompensation = Get-IliumUserPathSnapshot
            $receipt.path_after_compensation = $afterCompensation
            $receipt.path_restored = Test-IliumSameUserPath $afterCompensation $originalPath
            if (-not $receipt.path_restored) { throw 'User PATH compensation readback differs.' }
        } catch { $errors.Add('PATH: ' + $_.Exception.Message) }
    }
} catch { $errors.Add('preflight: ' + $_.Exception.Message) }

if ($errors.Count -eq 0) { $receipt.state = 'passed' }
else { $receipt.errors = $errors.ToArray() }
if ($evidencePath -and [IO.Directory]::Exists($evidencePath)) {
    [IO.File]::WriteAllText((Join-Path $evidencePath 'receipt.json'), (($receipt | ConvertTo-Json -Depth 5) + "`n"), (New-Object Text.UTF8Encoding($false)))
}
$eventType = 'result'
if ($errors.Count -ne 0) { $eventType = 'error' }
$event = @{type=$eventType; command='windows-preview-installer-check'; state=$receipt.state;
    version=$expectedVersion; cleanup=$receipt.cleanup; path_restored_by_uninstall=$receipt.path_restored_by_uninstall;
    path_restored=$receipt.path_restored; path_compensation_applied=$receipt.path_compensation_applied; errors=$errors.ToArray()}
Write-Output ($event | ConvertTo-Json -Compress)
if ($errors.Count -ne 0) { exit 1 }
