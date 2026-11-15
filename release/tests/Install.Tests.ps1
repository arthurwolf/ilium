# Pester 5; invoke on Windows with both powershell.exe 5.1 and pwsh.exe.
# Filesystem writes belong to TestDrive; registry integration cases use isolated
# HKCU test keys. Network, the real User PATH and binary execution are mocked.
BeforeAll {
    $installer = Join-Path $PSScriptRoot '../install.ps1'
    $tokens = $null; $parseErrors = $null
    $ast = [Management.Automation.Language.Parser]::ParseFile($installer, [ref]$tokens, [ref]$parseErrors)
    if ($parseErrors.Count) { throw ($parseErrors | Out-String) }
    $functions = $ast.FindAll({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] }, $false)
    . ([scriptblock]::Create(($functions | ForEach-Object { $_.Extent.Text }) -join "`n"))
    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    function New-TestRelease([string]$Version, [string]$Mode = '') {
        $zipPath = Join-Path $TestDrive ('payload-' + [guid]::NewGuid().ToString('N') + '.zip')
        $file = [IO.File]::Open($zipPath, [IO.FileMode]::CreateNew)
        $zip = New-Object IO.Compression.ZipArchive($file, [IO.Compression.ZipArchiveMode]::Create)
        try {
            $names = @('VERSION', 'THIRD-PARTY.txt', 'ilium-server.exe', 'ilium.exe', 'ilium-animation-helper.exe', 'beach-1.0.0.iliumanim', 'carpet-1.0.0.iliumanim')
            if ($Mode -eq 'partial') { $names = @('VERSION', 'THIRD-PARTY.txt', 'ilium.exe') }
            if ($Mode -eq 'traversal') { $names += '../escape.exe' }
            if ($Mode -eq 'absolute') { $names += 'C:/escape.exe' }
            if ($Mode -eq 'unexpected') { $names += 'run.ps1' }
            if ($Mode -eq 'duplicate') { $names += 'ILIUM.EXE' }
            foreach ($name in $names) {
                $entry = $zip.CreateEntry('ilium-windows-x86_64/' + $name)
                if ($Mode -eq 'reparse') { $entry.ExternalAttributes = 1024 }
                $stream = $entry.Open()
                try {
                    $data = [Text.Encoding]::UTF8.GetBytes("fixture-$name-$Version")
                    if ($name.EndsWith('.iliumanim')) { $data = [IO.File]::ReadAllBytes((Join-Path $PSScriptRoot ('../../ilium-animation-js/assets/packages/' + $name))) }
                    if ($name -eq 'beach-1.0.0.iliumanim' -and $Mode -eq 'tampered-animation') { $data = [Text.Encoding]::UTF8.GetBytes('tampered package') }
                    if ($name -eq 'VERSION') { $data = [Text.Encoding]::UTF8.GetBytes("$Version`n") }
                    if ($name -eq 'VERSION' -and $Mode -eq 'version') { $data = [Text.Encoding]::UTF8.GetBytes("99.0.0`n") }
                    $stream.Write($data, 0, $data.Length)
                } finally { $stream.Dispose() }
            }
        } finally { $zip.Dispose(); $file.Dispose() }
        return $zipPath
    }
    function Install-TestVersion([string]$Version, [string]$Mode = '') {
        $script:fixtureArchive = New-TestRelease $Version $Mode
        Invoke-IliumInstall -Version $Version -InstallDir $script:root -BinDir $script:bin
    }
}

Describe 'Windows installer transaction contract' -Skip:([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    BeforeEach {
        $script:root = Join-Path $TestDrive ('root-' + [guid]::NewGuid().ToString('N'))
        $script:bin = Join-Path $TestDrive ('bin-' + [guid]::NewGuid().ToString('N'))
        $script:userPath = 'C:\other;;C:\keep'
        $script:downloads = 0
        Mock Get-IliumNativeArchitecture { 'AMD64' }
        Mock Get-IliumUserPath { $script:userPath }
        Mock Set-IliumUserPath { param($Value) $script:userPath = $Value }
        Mock Confirm-IliumBinaryVersion { }
        Mock Get-IliumSigningStatus { 'unsigned' }
        Mock Save-IliumDownload {
            param($Url, $Destination)
            $script:downloads++
            if ($Url.EndsWith('/SHA256SUMS')) {
                $hash = (Get-FileHash -LiteralPath $script:fixtureArchive -Algorithm SHA256).Hash.ToLowerInvariant()
                $rows = @('ilium-linux-x86_64.tar.gz', 'ilium-linux-aarch64.tar.gz', 'ilium-windows-x86_64.zip', 'ilium-macos-aarch64.tar.gz', 'ilium-macos-x86_64.tar.gz')
                [IO.File]::WriteAllText($Destination, (($rows | ForEach-Object { "$hash  $_" }) -join "`n") + "`n", (New-Object Text.UTF8Encoding($false)))
            } else { [IO.File]::Copy($script:fixtureArchive, $Destination) }
        }
    }
    It 'selects AMD64 and installs a complete pair with one current pointer' {
        Install-TestVersion '1.0.0'
        [IO.File]::ReadAllText((Join-Path $script:root 'current')) | Should -Be "1.0.0`n"
        Test-Path (Join-Path $script:root 'versions/1.0.0/bin/ilium-server.exe') | Should -BeTrue
        Test-Path (Join-Path $script:bin 'ilium.cmd') | Should -BeTrue
        $script:userPath | Should -Be ('C:\other;;C:\keep;' + $script:bin)
    }
    It 'rejects <Arch> before download' -TestCases @(@{Arch='ARM64'}, @{Arch='x86'}) {
        param($Arch)
        $script:testArchitecture = $Arch
        Mock Get-IliumNativeArchitecture { $script:testArchitecture }
        { Install-TestVersion '1.0.0' } | Should -Throw '*Unsupported*'
        $script:downloads | Should -Be 0
        Test-Path $script:root | Should -BeFalse
    }
    It 'preserves TLS process state after a failed download' {
        $before = [Net.ServicePointManager]::SecurityProtocol
        $script:downloadTls = $null
        Mock Save-IliumDownload { $script:downloadTls = [Net.ServicePointManager]::SecurityProtocol; throw 'network or rate limit failure' }
        { Install-TestVersion '1.0.0' } | Should -Throw '*network*'
        ($script:downloadTls -band [Net.SecurityProtocolType]::Tls12) | Should -Be ([Net.SecurityProtocolType]::Tls12)
        ($script:downloadTls -band ([Net.SecurityProtocolType]::Ssl3 -bor [Net.SecurityProtocolType]::Tls -bor [Net.SecurityProtocolType]::Tls11)) | Should -Be 0
        [Net.ServicePointManager]::SecurityProtocol | Should -Be $before
    }
    It 'pins the recovery command to the resolved release after a download failure' {
        Install-TestVersion '1.0.0'
        $beforePath = $script:userPath
        Mock Get-IliumLatestVersion { '2.0.0' }
        Mock Save-IliumDownload { throw 'network failure' }
        $previousError = [Console]::Error
        $capturedError = New-Object IO.StringWriter
        try {
            [Console]::SetError($capturedError)
            { Invoke-IliumInstall -Version 'latest' -InstallDir $script:root -BinDir $script:bin } | Should -Throw '*network failure*'
        } finally {
            [Console]::SetError($previousError)
        }
        $capturedError.ToString() | Should -Match "-Version '2\.0\.0'"
        [IO.File]::ReadAllText((Join-Path $script:root 'current')) | Should -Be "1.0.0`n"
        $script:userPath | Should -Be $beforePath
        $capturedError.Dispose()
    }
    It 'rejects unsafe <Mode> archive without selecting it' -TestCases @(@{Mode='traversal'}, @{Mode='absolute'}, @{Mode='duplicate'}, @{Mode='unexpected'}, @{Mode='partial'}, @{Mode='reparse'}, @{Mode='version'}, @{Mode='tampered-animation'}) {
        param($Mode)
        Install-TestVersion '1.0.0'
        { Install-TestVersion '2.0.0' $Mode } | Should -Throw
        [IO.File]::ReadAllText((Join-Path $script:root 'current')) | Should -Be "1.0.0`n"
    }
    It 'rejects checksum corruption without executing archive code' {
        Install-TestVersion '1.0.0'
        Mock Save-IliumDownload {
            param($Url, $Destination)
            if ($Url.EndsWith('/SHA256SUMS')) {
                $rows = (Get-IliumTarget 'AMD64').checksum_archives | ForEach-Object { ('0' * 64) + '  ' + $_ }
                [IO.File]::WriteAllText($Destination, ($rows -join "`n") + "`n")
            }
            else { [IO.File]::Copy($script:fixtureArchive, $Destination) }
        }
        { Install-TestVersion '2.0.0' } | Should -Throw '*Archive checksum mismatch*'
        [IO.File]::ReadAllText((Join-Path $script:root 'current')) | Should -Be "1.0.0`n"
    }
    It 'installs a declared partial release and rejects a changed checksum manifest' {
        $script:fixtureArchive = New-TestRelease '1.0.0'
        $script:badPartialChecksum = $false
        Mock Save-IliumDownload {
            param($Url, $Destination)
            $hash = (Get-FileHash -LiteralPath $script:fixtureArchive -Algorithm SHA256).Hash.ToLowerInvariant()
            $sums = "$hash  ilium-linux-x86_64.tar.gz`n$hash  ilium-windows-x86_64.zip`n"
            $bytes = [Text.Encoding]::ASCII.GetBytes($sums)
            $algorithm = [Security.Cryptography.SHA256]::Create()
            try { $digest = ([BitConverter]::ToString($algorithm.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant() }
            finally { $algorithm.Dispose() }
            if ($Url.EndsWith('/SHA256SUMS')) {
                if ($script:badPartialChecksum) { $sums = $sums.Replace($hash, ('0' * 64)) }
                [IO.File]::WriteAllText($Destination, $sums, [Text.Encoding]::ASCII)
            } elseif ($Url.EndsWith('/AVAILABILITY')) {
                $rows = @('ilium-availability-v1', 'tag v1.0.0', ('commit ' + ('a' * 40)),
                    ('selection-sha256 ' + ('b' * 64)), ('checksums-sha256 ' + $digest),
                    'available ilium-linux-x86_64.tar.gz', 'omitted ilium-linux-aarch64.tar.gz',
                    'available ilium-windows-x86_64.zip', 'omitted ilium-macos-aarch64.tar.gz',
                    'omitted ilium-macos-x86_64.tar.gz')
                [IO.File]::WriteAllText($Destination, (($rows -join "`n") + "`n"), [Text.Encoding]::ASCII)
            } else { [IO.File]::Copy($script:fixtureArchive, $Destination) }
        }
        Invoke-IliumInstall -Version '1.0.0' -InstallDir $script:root -BinDir $script:bin
        [IO.File]::ReadAllText((Join-Path $script:root 'current')) | Should -Be "1.0.0`n"
        $script:badPartialChecksum = $true
        { Invoke-IliumInstall -Version '1.0.0' -InstallDir $script:root -BinDir $script:bin } | Should -Throw
        [IO.File]::ReadAllText((Join-Path $script:root 'current')) | Should -Be "1.0.0`n"
    }
    It 'supports spaces and Unicode in owned directories' {
        $script:root = Join-Path $TestDrive 'Ilium fixture 日本語'
        $script:bin = Join-Path $TestDrive 'launcher space é'
        Install-TestVersion '1.0.0'
        Test-Path (Join-Path $script:bin 'ilium-server.cmd') | Should -BeTrue
    }
    It 'dedupes normalized user PATH without claiming a pre-existing entry' {
        $script:userPath = 'C:\keep;' + $script:bin.ToUpperInvariant() + '\'
        $before = $script:userPath
        Install-TestVersion '1.0.0'
        $script:userPath | Should -Be $before
        Invoke-IliumInstall -Uninstall -InstallDir $script:root -BinDir $script:bin
        $script:userPath | Should -Be $before
    }
    It 'restores the original <Scenario> user PATH value and registry kind' -TestCases @(
        @{Scenario='expandable with trailing separator'; Exists=$true; Kind='ExpandString'; Value='%USERPROFILE%\.cargo\bin;'; Appended=''},
        @{Scenario='expandable with concurrent appended entry'; Exists=$true; Kind='ExpandString'; Value='%USERPROFILE%\.cargo\bin;'; Appended=';C:\authored'},
        @{Scenario='absent'; Exists=$false; Kind=$null; Value=$null; Appended=''},
        @{Scenario='empty'; Exists=$true; Kind='String'; Value=''; Appended=''}
    ) {
        param($Scenario, $Exists, $Kind, $Value, $Appended)
        $subkey = 'Software\IliumInstallerTests\' + [guid]::NewGuid().ToString('N')
        $script:pathKey = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($subkey)
        try {
            if ($Exists) { $script:pathKey.SetValue('Path', $Value, [Microsoft.Win32.RegistryValueKind]$Kind) }
            $script:pathJournalsAtWrite = @()
            Mock Get-IliumUserPath { Get-IliumRegistryPath $script:pathKey }
            Mock Set-IliumUserPath {
                param($Value)
                $journalPath = Join-Path $script:root 'installer-state/transaction.json'
                $script:pathJournalsAtWrite += ,([IO.File]::ReadAllText($journalPath) | ConvertFrom-Json)
                Set-IliumRegistryPath $script:pathKey $Value
            }

            Install-TestVersion '1.0.0'
            ($script:pathKey.GetValueNames() -contains 'Path') | Should -BeTrue
            if ($Exists) { $script:pathKey.GetValueKind('Path').ToString() | Should -BeExactly $Kind }
            $state = [IO.File]::ReadAllText((Join-Path $script:root 'installer-state/state.json')) | ConvertFrom-Json
            $state.path_originally_absent | Should -Be (-not $Exists)
            if (-not $Exists) {
                $script:pathJournalsAtWrite.Count | Should -Be 1
                ($null -eq $script:pathJournalsAtWrite[0].path_before) | Should -BeTrue
            }
            if ($Appended) {
                $installedPath = $script:pathKey.GetValue('Path', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
                $script:pathKey.SetValue('Path', ($installedPath + $Appended), [Microsoft.Win32.RegistryValueKind]$Kind)
            }

            Invoke-IliumInstall -Uninstall -InstallDir $script:root -BinDir $script:bin
            if (-not $Exists) {
                $script:pathJournalsAtWrite.Count | Should -Be 2
                ($null -eq $script:pathJournalsAtWrite[1].path_after) | Should -BeTrue
            }
            ($script:pathKey.GetValueNames() -contains 'Path') | Should -Be $Exists
            if ($Exists) {
                $script:pathKey.GetValueKind('Path').ToString() | Should -BeExactly $Kind
                $script:pathKey.GetValue('Path', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) |
                    Should -BeExactly ($Value + $Appended)
            }
        } finally {
            $script:pathKey.Dispose()
            [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($subkey)
        }
    }
    It 'is idempotent and retains current plus previous on upgrade' {
        Install-TestVersion '1.0.0'
        Install-TestVersion '1.0.0'
        Install-TestVersion '2.0.0'
        Install-TestVersion '3.0.0'
        Test-Path (Join-Path $script:root 'versions/1.0.0') | Should -BeFalse
        Test-Path (Join-Path $script:root 'versions/2.0.0/bin/ilium.exe') | Should -BeTrue
        Test-Path (Join-Path $script:root 'versions/3.0.0/bin/ilium.exe') | Should -BeTrue
    }
    It 'repairs a missing owned pair member on reinstall without replacing others' {
        Install-TestVersion '1.0.0'
        Remove-Item -LiteralPath (Join-Path $script:root 'versions/1.0.0/bin/ilium-server.exe')
        Install-TestVersion '1.0.0'
        Test-Path (Join-Path $script:root 'versions/1.0.0/bin/ilium-server.exe') | Should -BeTrue
    }
    It 'preserves modified binaries and refuses same-version overwrite' {
        Install-TestVersion '1.0.0'
        $file = Join-Path $script:root 'versions/1.0.0/bin/ilium.exe'
        [IO.File]::WriteAllText($file, 'authored bytes')
        { Install-TestVersion '1.0.0' } | Should -Throw '*modified*'
        [IO.File]::ReadAllText($file) | Should -Be 'authored bytes'
    }
    It 'provides rollback of PATH and keeps previous current when switch is locked' {
        Install-TestVersion '1.0.0'
        $script:userPath = 'C:\authored'
        $before = $script:userPath
        $pointer = Join-Path $script:root 'current'
        # Allow the initial pointer read; deny replacement at the activation step.
        $handle = [IO.File]::Open($pointer, 'Open', 'Read', 'Read')
        try { { Install-TestVersion '2.0.0' } | Should -Throw }
        finally { $handle.Dispose() }
        $script:userPath | Should -Be $before
        [IO.File]::ReadAllText($pointer) | Should -Be "1.0.0`n"
    }
    It 'keeps a concurrent authored PATH append when this transaction has not written PATH' {
        Install-TestVersion '1.0.0'
        $script:userPath = 'C:\authored'
        $script:pathReads = 0
        Mock Get-IliumUserPath {
            $script:pathReads++
            if ($script:pathReads -ge 2) { $script:userPath = 'C:\authored;' + $script:bin }
            $script:userPath
        }
        { Install-TestVersion '2.0.0' } | Should -Throw '*concurrently*'
        $script:userPath | Should -Be ('C:\authored;' + $script:bin)
        [IO.File]::ReadAllText((Join-Path $script:root 'current')) | Should -Be "1.0.0`n"
    }
    It 'keeps the active installation reachable when uninstall encounters a locked binary' {
        Install-TestVersion '1.0.0'
        $before = $script:userPath
        $file = Join-Path $script:root 'versions/1.0.0/bin/ilium.exe'
        $handle = [IO.File]::Open($file, 'Open', 'Read', 'Read')
        try { { Invoke-IliumInstall -Uninstall -InstallDir $script:root -BinDir $script:bin } | Should -Throw }
        finally { $handle.Dispose() }
        $script:userPath | Should -Be $before
        Test-Path (Join-Path $script:bin 'ilium.cmd') | Should -BeTrue
        Test-Path (Join-Path $script:bin 'ilium-server.cmd') | Should -BeTrue
        [IO.File]::ReadAllText((Join-Path $script:root 'current')) | Should -Be "1.0.0`n"
        Test-Path $file | Should -BeTrue
    }
    It 'rolls uninstall back when PATH removal is rejected after staging the pair' {
        Install-TestVersion '1.0.0'
        $before = $script:userPath
        Mock Set-IliumUserPath { throw 'PATH denied during uninstall' }
        { Invoke-IliumInstall -Uninstall -InstallDir $script:root -BinDir $script:bin } | Should -Throw '*PATH denied*'
        $script:userPath | Should -Be $before
        Test-Path (Join-Path $script:root 'versions/1.0.0/bin/ilium.exe') | Should -BeTrue
        Test-Path (Join-Path $script:bin 'ilium.cmd') | Should -BeTrue
        [IO.File]::ReadAllText((Join-Path $script:root 'current')) | Should -Be "1.0.0`n"
    }
    It 'restores the owned bin after a late uninstall failure without losing a concurrent PATH append' {
        Install-TestVersion '1.0.0'
        $script:atomicWriter = (Get-Item Function:Write-IliumAtomic).ScriptBlock
        $script:uninstallFaultInjected = $false
        Mock Write-IliumAtomic {
            param($Path, $Content, [switch]$NoReplace)
            if (-not $script:uninstallFaultInjected -and $Path -ceq (Join-Path $script:root 'installer-state/state.json') -and
                -not ($Content | ConvertFrom-Json).path_added) {
                # PATH removal is acknowledged, then an external actor appends
                # before the ownership-state write fails. Recovery runs for real.
                $script:uninstallFaultInjected = $true
                $script:userPath += ';C:\concurrent'
                throw 'late uninstall ownership write failure'
            }
            & $script:atomicWriter -Path $Path -Content $Content -NoReplace:$NoReplace
        }
        { Invoke-IliumInstall -Uninstall -InstallDir $script:root -BinDir $script:bin } | Should -Throw '*late uninstall*'
        $script:userPath | Should -Be ('C:\other;;C:\keep;C:\concurrent;' + $script:bin)
        [IO.File]::ReadAllText((Join-Path $script:root 'current')) | Should -Be "1.0.0`n"
        Test-Path (Join-Path $script:root 'versions/1.0.0/bin/ilium.exe') | Should -BeTrue
        Test-Path (Join-Path $script:root 'versions/1.0.0/bin/ilium-server.exe') | Should -BeTrue
        foreach ($name in @('ilium', 'ilium-server')) {
            [IO.File]::ReadAllText((Join-Path $script:bin ($name + '.cmd'))) | Should -Be (Get-IliumLauncher $script:root $name)
        }
        ([IO.File]::ReadAllText((Join-Path $script:root 'installer-state/state.json')) | ConvertFrom-Json).path_added | Should -BeTrue
        Test-Path (Join-Path $script:root 'installer-state/transaction.json') | Should -BeFalse
    }
    It 'leaves current active if antivirus or a locked launcher blocks publication' {
        Install-TestVersion '1.0.0'
        $launcher = Join-Path $script:bin 'ilium.cmd'
        $handle = [IO.File]::Open($launcher, 'Open', 'Read', 'None')
        try { { Install-TestVersion '2.0.0' } | Should -Throw }
        finally { $handle.Dispose() }
        [IO.File]::ReadAllText((Join-Path $script:root 'current')) | Should -Be "1.0.0`n"
    }
    It 'rejects a concurrent invocation through a real exclusive file lock' {
        Install-TestVersion '1.0.0'
        $handle = [IO.File]::Open((Join-Path $script:root '.install-lock'), 'Open', 'ReadWrite', 'None')
        try { { Install-TestVersion '2.0.0' } | Should -Throw '*another installer*' }
        finally { $handle.Dispose() }
    }
    It 'rejects invalid current pointer before changing anything' {
        Install-TestVersion '1.0.0'
        [IO.File]::WriteAllText((Join-Path $script:root 'current'), "../escape`n")
        { Install-TestVersion '2.0.0' } | Should -Throw '*pointer*'
    }
    It 'uninstall preserves config, unknown versions and modified launchers' {
        Install-TestVersion '1.0.0'
        $unknown = Join-Path $script:root 'versions/authored'
        [IO.Directory]::CreateDirectory($unknown) | Out-Null
        [IO.File]::WriteAllText((Join-Path $unknown 'keep'), 'keep')
        [IO.File]::WriteAllText((Join-Path $script:root 'config.toml'), 'keep')
        [IO.File]::WriteAllText((Join-Path $script:bin 'ilium.cmd'), 'keep')
        Invoke-IliumInstall -Uninstall -InstallDir $script:root -BinDir $script:bin
        Test-Path (Join-Path $unknown 'keep') | Should -BeTrue
        Test-Path (Join-Path $script:root 'config.toml') | Should -BeTrue
        [IO.File]::ReadAllText((Join-Path $script:bin 'ilium.cmd')) | Should -Be 'keep'
        Test-Path (Join-Path $script:root 'installer-state/state.json') | Should -BeTrue
        $script:userPath | Should -Be 'C:\other;;C:\keep'
    }
    It 'removes owned metadata and empty default directories after uninstall' {
        $script:bin = Join-Path $script:root 'bin'
        $script:fixtureArchive = New-TestRelease '1.0.0'
        Invoke-IliumInstall -Version '1.0.0' -InstallDir $script:root
        $authored = Join-Path $script:root 'config.toml'
        [IO.File]::WriteAllText($authored, 'keep')
        $before = $script:userPath

        Invoke-IliumInstall -Uninstall -InstallDir $script:root

        Test-Path (Join-Path $script:root 'installer-state/state.json') | Should -BeFalse
        Test-Path (Join-Path $script:root 'installer-state') | Should -BeFalse
        Test-Path (Join-Path $script:root 'versions') | Should -BeFalse
        Test-Path $script:bin | Should -BeFalse
        Test-Path $script:root | Should -BeTrue
        Test-Path (Join-Path $script:root '.install-lock') | Should -BeTrue
        [IO.File]::ReadAllText($authored) | Should -Be 'keep'
        # A completed uninstall removes only the PATH entry it added (as in the test above).
        $before | Should -Be ('C:\other;;C:\keep;' + $script:bin)
        $script:userPath | Should -Be 'C:\other;;C:\keep'
    }
    It 'preserves unknown installer metadata while removing empty default directories' {
        $script:bin = Join-Path $script:root 'bin'
        $script:fixtureArchive = New-TestRelease '1.0.0'
        Invoke-IliumInstall -Version '1.0.0' -InstallDir $script:root
        $unknown = Join-Path $script:root 'installer-state/authored.json'
        [IO.File]::WriteAllText($unknown, 'keep')

        Invoke-IliumInstall -Uninstall -InstallDir $script:root

        Test-Path $unknown | Should -BeTrue
        Test-Path (Join-Path $script:root 'installer-state/state.json') | Should -BeTrue
        Test-Path (Join-Path $script:root 'versions') | Should -BeFalse
        Test-Path $script:bin | Should -BeFalse
        [IO.File]::ReadAllText($unknown) | Should -Be 'keep'
        Test-Path (Join-Path $script:root '.install-lock') | Should -BeTrue
    }
    It 'rejects low disk space before any downloads' {
        Mock Get-IliumFreeSpace { 1024 }
        { Install-TestVersion '1.0.0' } | Should -Throw '*64 MiB*'
        $script:downloads | Should -Be 0
    }
    It 'refuses unowned or modified launchers before downloading' {
        [IO.Directory]::CreateDirectory($script:bin) | Out-Null
        $launcher = Join-Path $script:bin 'ilium.cmd'
        [IO.File]::WriteAllText($launcher, 'authored')
        { Install-TestVersion '1.0.0' } | Should -Throw '*modified launcher*'
        [IO.File]::ReadAllText($launcher) | Should -Be 'authored'
        $script:downloads | Should -Be 0
    }
    It 'honors NoModifyPath while installing owned launchers' {
        $script:fixtureArchive = New-TestRelease '1.0.0'
        Invoke-IliumInstall -Version '1.0.0' -InstallDir $script:root -BinDir $script:bin -NoModifyPath
        $script:userPath | Should -Be 'C:\other;;C:\keep'
        Test-Path (Join-Path $script:bin 'ilium.cmd') | Should -BeTrue
    }
    It 'preserves prior selection on PATH update rejection' {
        Install-TestVersion '1.0.0'
        $script:userPath = 'C:\outside'
        Mock Set-IliumUserPath { throw 'PATH denied' }
        { Install-TestVersion '2.0.0' } | Should -Throw '*PATH denied*'
        [IO.File]::ReadAllText((Join-Path $script:root 'current')) | Should -Be "1.0.0`n"
        $script:userPath | Should -Be 'C:\outside'
    }
    It 'recovers an interrupted journal before another installation' {
        Install-TestVersion '1.0.0'
        $statePath = Join-Path $script:root 'installer-state/state.json'
        $oldState = [IO.File]::ReadAllText($statePath)
        $journal = @{schema=1; operation='install'; ready=$false; next='2.0.0'; bin=$script:bin; path_added=$false; path_applied=$false;
            path_before=$script:userPath; path_after=$script:userPath; new_launchers=@(); old_state=$oldState}
        [IO.File]::WriteAllText((Join-Path $script:root 'installer-state/transaction.json'), ($journal | ConvertTo-Json -Compress))
        Install-TestVersion '2.0.0'
        Test-Path (Join-Path $script:root 'installer-state/transaction.json') | Should -BeFalse
        [IO.File]::ReadAllText((Join-Path $script:root 'current')) | Should -Be "2.0.0`n"
    }
    It 'rejects mismatched binary identity before activation' {
        Install-TestVersion '1.0.0'
        Mock Confirm-IliumBinaryVersion { throw 'binary identity mismatch' }
        { Install-TestVersion '2.0.0' } | Should -Throw '*identity mismatch*'
        [IO.File]::ReadAllText((Join-Path $script:root 'current')) | Should -Be "1.0.0`n"
    }
    It 'rejects invalid Authenticode and discloses an unsigned release' {
        $output = Install-TestVersion '1.0.0'
        ($output -join "`n") | Should -Match 'Authenticode: unsigned'
        Mock Get-IliumSigningStatus { throw 'Authenticode verification failed' }
        { Install-TestVersion '2.0.0' } | Should -Throw '*Authenticode*'
        [IO.File]::ReadAllText((Join-Path $script:root 'current')) | Should -Be "1.0.0`n"
    }
    It 'rejects reparse-point install paths without writing through them' {
        $destination = Join-Path $TestDrive 'destination'
        [IO.Directory]::CreateDirectory($destination) | Out-Null
        $junction = Join-Path $TestDrive 'junction'
        New-Item -ItemType Junction -Path $junction -Target $destination | Out-Null
        $script:root = Join-Path $junction 'ilium'
        { Install-TestVersion '1.0.0' } | Should -Throw '*reparse*'
        Test-Path (Join-Path $destination 'ilium') | Should -BeFalse
    }
}

Describe 'Pinned Windows preview selection and checksum' {
    It 'uses the published Windows preview only for a definite absence of a stable release' {
        (Resolve-IliumLatestResponse 302 'https://github.com/arthurwolf/ilium/releases') | Should -Be '0.1.1-preview.6'
        (Resolve-IliumLatestResponse 404 '') | Should -Be '0.1.1-preview.6'
        (Resolve-IliumLatestResponse 302 'https://github.com/arthurwolf/ilium/releases/tag/v0.2.0') | Should -Be '0.2.0'
        { Resolve-IliumLatestResponse 429 '' } | Should -Throw
        { Resolve-IliumLatestResponse 302 'https://github.com/arthurwolf/ilium/releases/tag/v0.2.0-preview.1' } | Should -Throw
        { Resolve-IliumLatestResponse 302 'https://evil.example/releases' } | Should -Throw
    }
    It 'accepts the published 17-asset checksum inventory only with the pinned Windows archive digest' {
        $manifest = Join-Path $PSScriptRoot 'fixtures/windows-preview-6.SHA256SUMS'
        (Get-IliumPreviewChecksum $manifest (Get-IliumTarget 'AMD64') '0.1.1-preview.6') |
            Should -Be 'c267349717786f9540ea06dd26a1976adcde4b9209340d12848e679c1239cb89'
    }
    It 'rejects modified, duplicate and missing preview checksum records' {
        $manifest = Join-Path $PSScriptRoot 'fixtures/windows-preview-6.SHA256SUMS'
        $content = [IO.File]::ReadAllText($manifest)
        $selectedLine = $content.Split("`n") | Where-Object { $_ -match 'ilium-windows-x86_64\.zip' } | Select-Object -First 1
        $withoutSelected = (($content.Split("`n") | Where-Object { $_ -and $_ -notmatch 'ilium-windows-x86_64\.zip' }) -join "`n") + "`n"
        foreach ($changed in @(
            $content.Replace('c267349717786f9540ea06dd26a1976adcde4b9209340d12848e679c1239cb89', ('0' * 64)),
            ($content + $selectedLine + "`n"),
            $withoutSelected
        )) {
            $path = Join-Path $TestDrive ([guid]::NewGuid().ToString('N') + '.txt')
            [IO.File]::WriteAllText($path, $changed)
            { Get-IliumPreviewChecksum $path (Get-IliumTarget 'AMD64') '0.1.1-preview.6' } | Should -Throw
        }
        { Get-IliumPreviewChecksum $manifest (Get-IliumTarget 'AMD64') '0.1.1-preview.5' } | Should -Throw '*not ready*'
    }
}
