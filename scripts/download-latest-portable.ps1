param(
    [Parameter(Mandatory = $true)]
    [string]$Owner,
    [Parameter(Mandatory = $true)]
    [string]$Repo,
    [string]$OutDir = ".\download"
)

$ErrorActionPreference = "Stop"

New-Item -ItemType Directory -Path $OutDir -Force | Out-Null

$api = "https://api.github.com/repos/$Owner/$Repo/releases/latest"
Write-Host "Fetching latest release from $api"
$release = Invoke-RestMethod -Uri $api -Headers @{ "User-Agent" = "OlehGroovyEditorDownloader" }

$asset = $release.assets | Where-Object { $_.name -eq "OlehGroovyEditor-windows-portable.zip" } | Select-Object -First 1
if (-not $asset) {
    throw "Asset OlehGroovyEditor-windows-portable.zip not found in latest release."
}

$zipPath = Join-Path $OutDir $asset.name
Write-Host "Downloading $($asset.browser_download_url)"
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zipPath -Headers @{ "User-Agent" = "OlehGroovyEditorDownloader" }

$extractDir = Join-Path $OutDir "OlehGroovyEditor"
if (Test-Path $extractDir) {
    Remove-Item $extractDir -Recurse -Force
}
Expand-Archive -Path $zipPath -DestinationPath $extractDir -Force

Write-Host "Downloaded: $zipPath"
Write-Host "Extracted to: $extractDir"
