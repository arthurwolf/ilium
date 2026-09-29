# Portable helper-contract verification; does not claim Windows-native behavior.
# All data belongs to a private temporary directory. JSONL is the CLI contract.
[CmdletBinding()]
param([string]$Installer = (Join-Path $PSScriptRoot '../install.ps1'), [switch]$TransactionRegressionsOnly)
$ErrorActionPreference = 'Stop'
$tokens = $null; $parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($Installer, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { @{type='error';stage='parse';errors=@($parseErrors | ForEach-Object {$_.Message})} | ConvertTo-Json -Compress; exit 1 }
$functions = $ast.FindAll({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst]}, $false)
. ([scriptblock]::Create(($functions | ForEach-Object {$_.Extent.Text}) -join "`n"))
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$temporary = Join-Path ([IO.Path]::GetTempPath()) ('ilium-windows-contracts-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($temporary) | Out-Null
$script:checks = 0; $script:failures = 0
function Assert-Contract([string]$Name, [scriptblock]$Check) {
    if ($TransactionRegressionsOnly -and -not $Name.StartsWith('transaction ')) { return }
    try { & $Check 3> $null; $script:checks++; @{type='result';test=$Name;state='passed'} | ConvertTo-Json -Compress }
    catch { $script:failures++; @{type='error';test=$Name;error=$_.Exception.Message} | ConvertTo-Json -Compress }
}
function Require([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Require-Throw([scriptblock]$Action) {
    $thrown = $false
    try { & $Action | Out-Null } catch { $thrown = $true }
    Require $thrown 'Expected failure did not occur.'
}
function Get-ContractRecoveryCommand([string]$Version, [string]$InstallDir, [string]$BinDir, [switch]$NoModifyPath, [switch]$Uninstall) {
    $writer = New-Object IO.StringWriter
    $previousError = [Console]::Error
    try {
        [Console]::SetError($writer)
        try { Invoke-IliumInstall -Version $Version -InstallDir $InstallDir -BinDir $BinDir -NoModifyPath:$NoModifyPath -Uninstall:$Uninstall | Out-Null }
        catch { } # The architecture adapter rejects before any I/O.
    } finally { [Console]::SetError($previousError) }
    $lines = @($writer.ToString().Split([char]10) | Where-Object { $_.StartsWith('ilium-install: recovery=') })
    Require ($lines.Count -eq 1) 'Missing unique recovery diagnostic.'
    return $lines[0].Substring('ilium-install: recovery='.Length).TrimEnd([char]13)
}
function New-ContractZip([string]$Mode) {
    $path = Join-Path $temporary ($Mode + '.zip')
    $file = [IO.File]::Open($path, 'CreateNew')
    $zip = New-Object IO.Compression.ZipArchive($file, [IO.Compression.ZipArchiveMode]::Create)
    try {
        $names = @('VERSION', 'THIRD-PARTY.txt', 'ilium-server.exe', 'ilium.exe')
        if ($Mode -eq 'partial') { $names = @('VERSION', 'THIRD-PARTY.txt', 'ilium.exe') }
        if ($Mode -eq 'traversal') { $names += '../outside' }
        if ($Mode -eq 'absolute') { $names += '/outside' }
        if ($Mode -eq 'duplicate') { $names += 'ilium.exe' }
        if ($Mode -eq 'case-duplicate') { $names += 'ILIUM.EXE' }
        if ($Mode -eq 'unexpected') { $names += 'activate.ps1' }
        if ($Mode -eq 'dll') { $names += 'onnxruntime.dll' }
        $root = $zip.CreateEntry('ilium-windows-x86_64/')
        $root.ExternalAttributes = 16
        foreach ($name in $names) {
            $entry = $zip.CreateEntry('ilium-windows-x86_64/' + $name)
            if ($Mode -eq 'reparse') { $entry.ExternalAttributes = 1024 }
            if ($Mode -eq 'symlink') { $entry.ExternalAttributes = -1610612736 }
            $stream = $entry.Open()
            try {
                $text = 'fixture'
                if ($name -eq 'VERSION') { $text = "1.0.0`n" }
                if ($name -eq 'VERSION' -and $Mode -eq 'version') { $text = "2.0.0`n" }
                $data = [Text.Encoding]::UTF8.GetBytes($text); $stream.Write($data, 0, $data.Length)
            } finally { $stream.Dispose() }
        }
    } finally { $zip.Dispose(); $file.Dispose() }
    return $path
}
function New-TransactionFixture {
    $root = Join-Path $temporary ([guid]::NewGuid().ToString('N'))
    $bin = Join-Path $root 'bin'
    $state = Join-Path $root 'installer-state'
    $pair = Join-Path $root 'versions/1.0.0/bin'
    [IO.Directory]::CreateDirectory($state) | Out-Null
    [IO.Directory]::CreateDirectory($pair) | Out-Null
    [IO.Directory]::CreateDirectory($bin) | Out-Null
    foreach ($name in @('ilium.exe', 'ilium-server.exe', 'VERSION', 'THIRD-PARTY.txt')) {
        [IO.File]::WriteAllText((Join-Path $pair $name), ('fixture-' + $name))
    }
    Write-IliumAtomic (Join-Path $state 'version-1.0.0.json') ((Get-IliumReceipt $pair) | ConvertTo-Json -Compress)
    $oldState = @{schema=1; owner='ilium-windows-installer-1'; root=$root; bin=$bin; path_added=$true; previous=''} | ConvertTo-Json -Compress
    Write-IliumAtomic (Join-Path $state 'state.json') $oldState
    Write-IliumAtomic (Join-Path $root 'current') "1.0.0`n"
    foreach ($name in @('ilium', 'ilium-server')) { Write-IliumAtomic (Join-Path $bin ($name + '.cmd')) (Get-IliumLauncher $root $name) }
    $journal = @{schema=1; operation='install'; ready=$false; next='2.0.0'; bin=$bin; path_added=$true; path_applied=$false; path_before='authored'; path_after=('authored;' + $bin); new_launchers=@(); old_state=$oldState}
    return @{root=$root; bin=$bin; state=$state; pair=$pair; journal=$journal}
}
try {
    Assert-Contract 'AMD64 manifest selection' { Require ((Get-IliumTarget 'AMD64').archive -ceq 'ilium-windows-x86_64.zip') 'Wrong archive.' }
    foreach ($arch in @('ARM64', 'x86', 'aarch64', '')) { Assert-Contract ('reject architecture ' + $arch) { Require-Throw {Get-IliumTarget $arch} } }
    foreach ($version in @('1.0.0', '0.1.0-beta.2')) { Assert-Contract ('accept version ' + $version) { Require (Test-IliumVersion $version) 'Rejected valid version.' } }
    foreach ($version in @('../x', 'C:\x', '', "1.0.0`n", '1.0.0/child', '1.0.0&whoami', '1.0.0..')) { Assert-Contract ('reject version ' + $version.Replace("`n", '\n')) { Require (-not (Test-IliumVersion $version)) 'Unsafe version accepted.' } }
    foreach ($mode in @('valid', 'dll')) {
        Assert-Contract ('extract valid archive ' + $mode) {
            $path = New-ContractZip $mode
            $destination = Join-Path $temporary ('extract-' + $mode)
            Expand-IliumValidatedZip $path $destination 'ilium-windows-x86_64' '1.0.0'
            Require ([IO.File]::Exists((Join-Path $destination 'ilium.exe'))) 'Client missing.'
            Require ([IO.File]::Exists((Join-Path $destination 'ilium-server.exe'))) 'Server missing.'
        }
    }
    foreach ($mode in @('traversal', 'absolute', 'duplicate', 'case-duplicate', 'unexpected', 'partial', 'reparse', 'symlink', 'version')) {
        Assert-Contract ('reject archive ' + $mode) {
            $path = New-ContractZip $mode
            Require-Throw { Expand-IliumValidatedZip $path (Join-Path $temporary ('extract-' + $mode)) 'ilium-windows-x86_64' '1.0.0' }
        }
    }
    $target = Get-IliumTarget 'AMD64'; $checksumFile = Join-Path $temporary 'SHA256SUMS'
    $rows = @($target.checksum_archives | ForEach-Object { ('a' * 64) + '  ' + $_ })
    [IO.File]::WriteAllText($checksumFile, ($rows -join "`n") + "`n")
    Assert-Contract 'select checksum from exact five-target inventory' { Require ((Get-IliumChecksum $checksumFile $target) -ceq ('a' * 64)) 'Wrong checksum.' }
    foreach ($mode in @('missing', 'duplicate', 'unexpected', 'uppercase', 'crlf')) {
        Assert-Contract ('reject checksum ' + $mode) {
            $bad = ($rows -join "`n") + "`n"
            if ($mode -eq 'missing') { $bad = $rows[2] + "`n" }
            if ($mode -eq 'duplicate') { $bad += $rows[2] + "`n" }
            if ($mode -eq 'unexpected') { $bad += ('a' * 64) + "  untrusted.zip`n" }
            if ($mode -eq 'uppercase') { $bad = $bad.Replace(('a' * 64), ('A' * 64)) }
            if ($mode -eq 'crlf') { $bad = $bad.Replace("`n", "`r`n") }
            [IO.File]::WriteAllText($checksumFile, $bad)
            Require-Throw { Get-IliumChecksum $checksumFile $target }
        }
    }
    Assert-Contract 'atomic pointer write preserves complete old or new bytes' {
        $pointer = Join-Path $temporary 'current'
        Write-IliumAtomic $pointer "1.0.0`n"
        Write-IliumAtomic $pointer "2.0.0`n"
        Require ([IO.File]::ReadAllText($pointer) -ceq "2.0.0`n") 'Pointer bytes differ.'
    }
    Assert-Contract 'launcher retains Unicode root and reads current once' {
        $root = 'C:\Users\日本語\Ilium space'
        $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($root))
        $launcher = Get-IliumLauncher $root 'ilium'
        Require ($launcher.Contains($encoded)) 'Root encoding differs.'
        Require (([regex]::Matches($launcher, '\[IO.File\]::ReadAllText')).Count -eq 1) 'Pointer read more than once.'
        Require ($launcher.Contains('ilium-server.exe')) 'Sibling pair not checked.'
    }
    Assert-Contract 'remove PATH only once and retain authored duplicate and empties' {
        Require ((Remove-IliumPathEntry 'C:\keep;;C:\bin;C:\bin;C:\other' 'C:\bin') -ceq 'C:\keep;;C:\bin;C:\other') 'Unrelated PATH entries changed.'
    }
    Assert-Contract 'owned version detects tamper and supports missing-file repair check' {
        $root = Join-Path $temporary 'owned'
        $bin = Join-Path $root 'versions/1.0.0/bin'
        $state = Join-Path $root 'installer-state'
        [IO.Directory]::CreateDirectory($state) | Out-Null
        Expand-IliumValidatedZip (Join-Path $temporary 'valid.zip') $bin 'ilium-windows-x86_64' '1.0.0'
        $receipt = Get-IliumReceipt $bin
        Write-IliumAtomic (Join-Path $state 'version-1.0.0.json') ($receipt | ConvertTo-Json -Compress)
        Require (Test-IliumOwnedVersion $root '1.0.0') 'Owned version rejected.'
        [IO.File]::Delete((Join-Path $bin 'ilium-server.exe'))
        Require (-not (Test-IliumOwnedVersion $root '1.0.0')) 'Incomplete pair accepted.'
        Require (Test-IliumOwnedVersion $root '1.0.0' -AllowMissing) 'Missing owned member cannot be repaired.'
        [IO.File]::WriteAllText((Join-Path $bin 'ilium.exe'), 'authored')
        Require (-not (Test-IliumOwnedVersion $root '1.0.0' -AllowMissing)) 'Modified file claimed.'
    }
    # Registry PATH is an external adapter. Actual journal, filesystem, hash and
    # recovery bodies run here; this remains portable evidence, not Windows proof.
    function Get-IliumUserPath { return $script:fixtureUserPath }
    function Set-IliumUserPath([AllowNull()][string]$Value) { $script:fixtureUserPath = $Value }
    function Get-IliumNativeArchitecture { return 'x86' }
    function Invoke-RestMethod([string]$Uri) {
        Require ($Uri -ceq 'https://ilium-setup.pages.dev/install.ps1') 'Recovery fetched an unexpected endpoint.'
        return 'param([string]$Version, [string]$InstallDir, [string]$BinDir, [switch]$NoModifyPath, [switch]$Uninstall) @{version=$Version;root=$InstallDir;bin=$BinDir;no_modify_path=[bool]$NoModifyPath;uninstall=[bool]$Uninstall}'
    }
    foreach ($mode in @(@{no_modify_path=$false;uninstall=$false}, @{no_modify_path=$true;uninstall=$false}, @{no_modify_path=$false;uninstall=$true}, @{no_modify_path=$true;uninstall=$true})) {
        Assert-Contract ('transaction recovery command executes with exact quoted arguments and independent switches ' + $mode.no_modify_path + '/' + $mode.uninstall) {
            $version = 'v1.2.3-beta.4'
            $root = "C:\Users\O'Brien 日本語\Ilium space"
            $bin = "C:\Users\O'Brien\bin'; `$script:recoveryInjected = `$true; #"
            $script:recoveryInjected = $false
            $command = Get-ContractRecoveryCommand -Version $version -InstallDir $root -BinDir $bin -NoModifyPath:$mode.no_modify_path -Uninstall:$mode.uninstall
            Require ($command.Contains('https://ilium-setup.pages.dev/install.ps1')) 'Recovery is not an HTTPS bootstrap.'
            $commandTokens = $null; $commandErrors = $null
            [Management.Automation.Language.Parser]::ParseInput($command, [ref]$commandTokens, [ref]$commandErrors) | Out-Null
            Require ($commandErrors.Count -eq 0) 'Recovery command cannot be parsed.'
            $forwarded = & ([scriptblock]::Create($command))
            Require ($forwarded.version -ceq $version) 'Recovery changed the exact requested version.'
            Require ($forwarded.root -ceq $root) 'Recovery changed the quoted Unicode install path.'
            Require ($forwarded.bin -ceq $bin) 'Recovery changed the exact bin path.'
            Require ($forwarded.no_modify_path -eq $mode.no_modify_path) 'Recovery changed NoModifyPath.'
            Require ($forwarded.uninstall -eq $mode.uninstall) 'Recovery changed Uninstall mode.'
            Require (-not $script:recoveryInjected) 'Quoted recovery data executed as source.'
        }
    }
    Assert-Contract 'transaction recovery command escapes even a rejected version as data' {
        $version = "v1.2.3'; `$script:recoveryInjected = `$true; #"
        $script:recoveryInjected = $false
        $command = Get-ContractRecoveryCommand -Version $version -InstallDir '' -BinDir ''
        $forwarded = & ([scriptblock]::Create($command))
        Require ($forwarded.version -ceq $version) 'Recovery changed the rejected version argument.'
        Require ($forwarded.root -ceq '' -and $forwarded.bin -ceq '') 'Recovery changed default directory arguments.'
        Require (-not $script:recoveryInjected) 'Rejected version text executed as source.'
    }
    Assert-Contract 'transaction intent cannot remove a concurrent authored PATH entry' {
        $fixture = New-TransactionFixture
        $script:fixtureUserPath = 'authored;' + $fixture.bin
        Write-IliumAtomic (Join-Path $fixture.state 'transaction.json') ($fixture.journal | ConvertTo-Json -Compress)
        Recover-IliumTransaction $fixture.root $fixture.bin
        Require ($script:fixtureUserPath -ceq ('authored;' + $fixture.bin)) 'An entry was removed before any installer PATH write.'
    }
    Assert-Contract 'transaction confirmed PATH write rolls back its exact snapshot' {
        $fixture = New-TransactionFixture
        $fixture.journal.path_applied = $true
        $script:fixtureUserPath = 'authored;' + $fixture.bin
        Write-IliumAtomic (Join-Path $fixture.state 'transaction.json') ($fixture.journal | ConvertTo-Json -Compress)
        Recover-IliumTransaction $fixture.root $fixture.bin
        Require ($script:fixtureUserPath -ceq 'authored') 'The confirmed append was not rolled back.'
    }
    Assert-Contract 'transaction rollback preserves subsequent authored PATH edits' {
        $fixture = New-TransactionFixture
        $fixture.journal.path_applied = $true
        $script:fixtureUserPath = 'authored;' + $fixture.bin + ';concurrent'
        Write-IliumAtomic (Join-Path $fixture.state 'transaction.json') ($fixture.journal | ConvertTo-Json -Compress)
        Recover-IliumTransaction $fixture.root $fixture.bin
        Require ($script:fixtureUserPath -ceq ('authored;' + $fixture.bin + ';concurrent')) 'Concurrent PATH bytes changed.'
    }
    Assert-Contract 'transaction interrupted uninstall restores pair pointer launchers and PATH' {
        $fixture = New-TransactionFixture
        $fixture.journal.operation = 'uninstall'
        $fixture.journal.quarantine = 'uninstall-' + [guid]::NewGuid().ToString('N')
        $fixture.journal.versions = @('1.0.0')
        $fixture.journal.launchers = @('ilium', 'ilium-server')
        $fixture.journal.old_pointer = "1.0.0`n"
        $fixture.journal.path_before = 'authored;' + $fixture.bin
        $fixture.journal.path_after = 'authored'
        $fixture.journal.path_applied = $true
        $quarantine = Join-Path $fixture.state $fixture.journal.quarantine
        [IO.Directory]::CreateDirectory((Join-Path $quarantine 'versions')) | Out-Null
        [IO.Directory]::CreateDirectory((Join-Path $quarantine 'launchers')) | Out-Null
        [IO.Directory]::Move([IO.Directory]::GetParent($fixture.pair).FullName, (Join-Path $quarantine 'versions/1.0.0'))
        foreach ($name in $fixture.journal.launchers) { [IO.File]::Move((Join-Path $fixture.bin ($name + '.cmd')), (Join-Path $quarantine ('launchers/' + $name + '.cmd'))) }
        [IO.File]::Delete((Join-Path $fixture.root 'current'))
        $script:fixtureUserPath = 'authored'
        Write-IliumAtomic (Join-Path $fixture.state 'transaction.json') ($fixture.journal | ConvertTo-Json -Compress)
        Recover-IliumTransaction $fixture.root $fixture.bin
        Require (Test-IliumOwnedVersion $fixture.root '1.0.0') 'The staged pair was not restored.'
        Require ([IO.File]::ReadAllText((Join-Path $fixture.root 'current')) -ceq "1.0.0`n") 'The previous pointer was not restored.'
        Require ([IO.File]::Exists((Join-Path $fixture.bin 'ilium.cmd'))) 'The previous launcher was not restored.'
        Require ($script:fixtureUserPath -ceq ('authored;' + $fixture.bin)) 'The previous PATH was not restored.'
        Require (-not [IO.File]::Exists((Join-Path $fixture.state 'transaction.json'))) 'Recovered journal remains.'
    }
    Assert-Contract 'transaction interrupted uninstall restores reachability without altering concurrent PATH edits' {
        $fixture = New-TransactionFixture
        $fixture.journal.operation = 'uninstall'
        $fixture.journal.quarantine = 'uninstall-' + [guid]::NewGuid().ToString('N')
        $fixture.journal.versions = @('1.0.0')
        $fixture.journal.launchers = @('ilium', 'ilium-server')
        $fixture.journal.old_pointer = "1.0.0`n"
        $fixture.journal.path_before = 'authored;;' + $fixture.bin + ';last'
        $fixture.journal.path_after = 'authored;;last'
        $fixture.journal.path_applied = $true
        $quarantine = Join-Path $fixture.state $fixture.journal.quarantine
        [IO.Directory]::CreateDirectory((Join-Path $quarantine 'versions')) | Out-Null
        [IO.Directory]::CreateDirectory((Join-Path $quarantine 'launchers')) | Out-Null
        [IO.Directory]::Move([IO.Directory]::GetParent($fixture.pair).FullName, (Join-Path $quarantine 'versions/1.0.0'))
        foreach ($name in $fixture.journal.launchers) { [IO.File]::Move((Join-Path $fixture.bin ($name + '.cmd')), (Join-Path $quarantine ('launchers/' + $name + '.cmd'))) }
        [IO.File]::Delete((Join-Path $fixture.root 'current'))
        $script:fixtureUserPath = 'authored;;last;concurrent;'
        Write-IliumAtomic (Join-Path $fixture.state 'transaction.json') ($fixture.journal | ConvertTo-Json -Compress)
        Recover-IliumTransaction $fixture.root $fixture.bin
        Require (Test-IliumOwnedVersion $fixture.root '1.0.0') 'The staged pair was not restored.'
        Require ([IO.File]::ReadAllText((Join-Path $fixture.root 'current')) -ceq "1.0.0`n") 'The previous pointer was not restored.'
        foreach ($name in @('ilium', 'ilium-server')) { Require ([IO.File]::Exists((Join-Path $fixture.bin ($name + '.cmd')))) 'A previous launcher was not restored.' }
        Require ($script:fixtureUserPath -ceq ('authored;;last;concurrent;;' + $fixture.bin)) 'Owned bin was not restored while preserving concurrent PATH bytes and order.'
        Require (-not [IO.File]::Exists((Join-Path $fixture.state 'transaction.json'))) 'Recovered journal remains.'
    }
    Assert-Contract 'transaction uninstall rollback dedupes a concurrently restored normalized bin entry' {
        $fixture = New-TransactionFixture
        $fixture.journal.operation = 'uninstall'
        $fixture.journal.path_before = 'authored;' + $fixture.bin
        $fixture.journal.path_after = 'authored'
        $fixture.journal.path_applied = $true
        $script:fixtureUserPath = 'authored;"' + $fixture.bin + '/";concurrent'
        $before = $script:fixtureUserPath
        Restore-IliumTransactionPath $fixture.journal
        Require ($script:fixtureUserPath -ceq $before) 'An equivalent concurrent authored entry was duplicated or changed.'
    }
    Assert-Contract 'transaction uninstall rollback cannot restore an unowned bin entry' {
        $fixture = New-TransactionFixture
        $fixture.journal.operation = 'uninstall'
        $fixture.journal.path_before = 'authored'
        $fixture.journal.path_after = 'authored'
        $fixture.journal.path_applied = $true
        $script:fixtureUserPath = 'authored;concurrent'
        Restore-IliumTransactionPath $fixture.journal
        Require ($script:fixtureUserPath -ceq 'authored;concurrent') 'Rollback added an entry absent from the ownership snapshot.'
    }
    Assert-Contract 'transaction rejects a malformed uninstall quarantine before mutation' {
        $fixture = New-TransactionFixture
        $fixture.journal.operation = 'uninstall'
        $fixture.journal.quarantine = '../authored'
        $fixture.journal.versions = @('1.0.0')
        $fixture.journal.launchers = @()
        $fixture.journal.old_pointer = "1.0.0`n"
        $script:fixtureUserPath = 'authored;' + $fixture.bin
        Write-IliumAtomic (Join-Path $fixture.state 'transaction.json') ($fixture.journal | ConvertTo-Json -Compress)
        Require-Throw { Recover-IliumTransaction $fixture.root $fixture.bin }
        Require (Test-IliumOwnedVersion $fixture.root '1.0.0') 'Malformed journal changed the pair.'
        Require ($script:fixtureUserPath -ceq ('authored;' + $fixture.bin)) 'Malformed journal changed PATH.'
    }
    Assert-Contract 'transaction committed uninstall deletes only staged owned files' {
        $fixture = New-TransactionFixture
        $fixture.journal.operation = 'uninstall'
        $fixture.journal.ready = $true
        $fixture.journal.quarantine = 'uninstall-' + [guid]::NewGuid().ToString('N')
        $fixture.journal.versions = @('1.0.0')
        $fixture.journal.launchers = @()
        $fixture.journal.old_pointer = "1.0.0`n"
        $quarantine = Join-Path $fixture.state $fixture.journal.quarantine
        [IO.Directory]::CreateDirectory((Join-Path $quarantine 'versions')) | Out-Null
        [IO.Directory]::CreateDirectory((Join-Path $quarantine 'launchers')) | Out-Null
        [IO.Directory]::Move([IO.Directory]::GetParent($fixture.pair).FullName, (Join-Path $quarantine 'versions/1.0.0'))
        [IO.File]::Delete((Join-Path $fixture.root 'current'))
        [IO.File]::WriteAllText((Join-Path $fixture.root 'authored'), 'keep')
        $script:fixtureUserPath = 'authored'
        Write-IliumAtomic (Join-Path $fixture.state 'transaction.json') ($fixture.journal | ConvertTo-Json -Compress)
        Recover-IliumTransaction $fixture.root $fixture.bin
        Require (-not [IO.Directory]::Exists($quarantine)) 'Staged owned files remain after successful cleanup.'
        Require (-not [IO.File]::Exists((Join-Path $fixture.state 'version-1.0.0.json'))) 'Removed version receipt remains.'
        Require ([IO.File]::ReadAllText((Join-Path $fixture.root 'authored')) -ceq 'keep') 'Uninstall removed authored content.'
        Require (-not [IO.File]::Exists((Join-Path $fixture.root 'current'))) 'Cleanup reactivated an uninstalled pair.'
        Require ($script:fixtureUserPath -ceq 'authored') 'Cleanup changed user PATH.'
    }
} finally { [IO.Directory]::Delete($temporary, $true) }
@{type='summary';passed=$script:checks;failed=$script:failures;native_windows='unverified';powershell_version=$PSVersionTable.PSVersion.ToString()} | ConvertTo-Json -Compress
if ($script:failures) { exit 1 }
