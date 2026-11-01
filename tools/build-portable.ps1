$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$release = Join-Path $root "src-tauri\target\release"
$portable = Join-Path $release "portable"
$exe = Join-Path $release "ps3xmb.exe"

$running = Get-CimInstance Win32_Process | Where-Object {
    $_.ExecutablePath -and $_.ExecutablePath -ieq $exe
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

& robocopy (Join-Path $root "content") (Join-Path $portable "content") /E /COPY:DAT /DCOPY:DAT /R:2 /W:1
if ($LASTEXITCODE -ge 8) {
    throw "Copying the content folder failed with robocopy exit code $LASTEXITCODE."
}

$resources = Join-Path $root "src-tauri\resources"
if (Test-Path -LiteralPath $resources -PathType Container) {
    & robocopy $resources (Join-Path $portable "resources") /E /COPY:DAT /DCOPY:DAT /R:2 /W:1
    if ($LASTEXITCODE -ge 8) {
        throw "Copying runtime resources failed with robocopy exit code $LASTEXITCODE."
    }
}

Write-Output "Portable MaXMB folder: $portable"
