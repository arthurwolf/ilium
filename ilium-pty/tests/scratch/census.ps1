# SCRATCH DIAGNOSTIC (never on the release branch): under a real ConPTY,
# write each UI glyph at column 0 and read conhost's own cursor column back.
param([string]$Glyphs, [string]$Out)
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$lines = [System.IO.File]::ReadAllLines($Glyphs, [System.Text.Encoding]::UTF8)
$results = New-Object System.Collections.Generic.List[string]
foreach ($glyph in $lines) {
    if ($glyph.Length -eq 0) { continue }
    [Console]::SetCursorPosition(0, 0)
    [Console]::Write((' ' * 20))
    [Console]::SetCursorPosition(0, 0)
    [Console]::Write($glyph)
    $column = [Console]::CursorLeft
    $codes = ($glyph.EnumerateRunes() | ForEach-Object { 'U+{0:X4}' -f $_.Value }) -join ' '
    $results.Add("$codes`t$column")
}
[System.IO.File]::WriteAllLines($Out, $results)
