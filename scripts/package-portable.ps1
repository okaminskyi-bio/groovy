param(
    [string]$Configuration = "release"
)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
Push-Location $root

cargo build --$Configuration

$distRoot = Join-Path $root "dist"
$appDir = Join-Path $distRoot "OlehGroovyEditor"
New-Item -ItemType Directory -Path $appDir -Force | Out-Null

$exePath = Join-Path $root "target\$Configuration\oleh-groovy-editor.exe"
if (!(Test-Path $exePath)) {
    throw "Executable not found at $exePath"
}

Copy-Item $exePath (Join-Path $appDir "oleh-groovy-editor.exe") -Force
Copy-Item (Join-Path $root "portable\OlehGroovyEditor.bat") (Join-Path $appDir "OlehGroovyEditor.bat") -Force
Copy-Item (Join-Path $root "README.md") (Join-Path $appDir "README.md") -Force

$zipPath = Join-Path $distRoot "OlehGroovyEditor-windows-portable.zip"
if (Test-Path $zipPath) { Remove-Item $zipPath -Force }
Compress-Archive -Path "$appDir\*" -DestinationPath $zipPath -Force

Write-Host "Portable package created at $zipPath"

Pop-Location
