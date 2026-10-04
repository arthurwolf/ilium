# Pester 5; invoke on Windows with both powershell.exe 5.1 and pwsh.exe.
# Every write belongs to TestDrive. Network, User PATH and binary execution are
# mocked at adapters; archive, hashes, locking and filesystem transactions are real.
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
        $script:userPath | Should -Be 'C:\other;;C:\keep'
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
