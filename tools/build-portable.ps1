param(
    [string]$OutputPath
)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$release = Join-Path $root "src-tauri\target\release"
$portable = if ($OutputPath) {
    [System.IO.Path]::GetFullPath($OutputPath)
} else {
    Join-Path (Split-Path -Parent $root) "MaXMB-Portable"
}
$exe = Join-Path $release "ps3xmb.exe"
$sourceRoot = [System.IO.Path]::GetFullPath($root).TrimEnd('\')
$portableRoot = [System.IO.Path]::GetFullPath($portable).TrimEnd('\')

if ($portableRoot.Equals($sourceRoot, [System.StringComparison]::OrdinalIgnoreCase) -or
    $portableRoot.StartsWith($sourceRoot + '\', [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "The portable output must be outside the development project: $portableRoot"
}

$running = Get-CimInstance Win32_Process | Where-Object {
    $_.ExecutablePath -and (
        $_.ExecutablePath -ieq $exe -or
        $_.ExecutablePath.StartsWith($portableRoot + '\', [System.StringComparison]::OrdinalIgnoreCase)
    )
}
if ($running) {
    throw "Close the release app before rebuilding the portable folder."
}

Push-Location $root
try {
    & npm.cmd run build -- --no-bundle
    if ($LASTEXITCODE -ne 0) {
        throw "The portable release build failed with exit code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
}

if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) {
    throw "The release executable was not produced: $exe"
}

New-Item -ItemType Directory -Path $portable -Force | Out-Null
Copy-Item -LiteralPath $exe -Destination (Join-Path $portable "ps3xmb.exe") -Force
Copy-Item -LiteralPath (Join-Path $root "MaXMB Manual.pdf") -Destination $portable -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot "PORTABLE_README.txt") -Destination $portable -Force

$portableContent = Join-Path $portable "content"
if (-not (Test-Path -LiteralPath $portableContent -PathType Container)) {
    & robocopy (Join-Path $root "content") $portableContent /E /COPY:DAT /DCOPY:DAT /R:2 /W:1
    if ($LASTEXITCODE -ge 8) {
        throw "Seeding the portable content snapshot failed with robocopy exit code $LASTEXITCODE."
    }
}

$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
$jsonFiles = Get-ChildItem -LiteralPath $portableContent -Filter "*.json" -File -Recurse |
    Where-Object { $_.FullName -notlike (Join-Path $portableContent "data\webview") + "\*" }
$sourceRootForward = $sourceRoot.Replace('\', '/')
$portableRootForward = $portableRoot.Replace('\', '/').Replace('$', '$$')
$portableRootWindows = $portableRoot.Replace('$', '$$')
foreach ($file in $jsonFiles) {
    $text = [System.IO.File]::ReadAllText($file.FullName)
    $text = [System.Text.RegularExpressions.Regex]::Replace(
        $text,
        [System.Text.RegularExpressions.Regex]::Escape($sourceRoot),
        $portableRootWindows,
        [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
    )
    $text = [System.Text.RegularExpressions.Regex]::Replace(
        $text,
        [System.Text.RegularExpressions.Regex]::Escape($sourceRootForward),
        $portableRootForward,
        [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
    )
    [System.IO.File]::WriteAllText($file.FullName, $text, $utf8NoBom)
}

$bundledContent = Join-Path $release "content"
if (Test-Path -LiteralPath $bundledContent -PathType Container) {
    & robocopy $bundledContent $portableContent /E /COPY:DAT /DCOPY:DAT /R:2 /W:1
    if ($LASTEXITCODE -ge 8) {
        throw "Copying bundled runtime content failed with robocopy exit code $LASTEXITCODE."
    }
}

$resources = Join-Path $root "src-tauri\resources"
if (Test-Path -LiteralPath $resources -PathType Container) {
    & robocopy $resources (Join-Path $portable "resources") /E /COPY:DAT /DCOPY:DAT /R:2 /W:1
    if ($LASTEXITCODE -ge 8) {
        throw "Copying runtime resources failed with robocopy exit code $LASTEXITCODE."
    }
}

Write-Output "Independent portable MaXMB folder: $portable"
