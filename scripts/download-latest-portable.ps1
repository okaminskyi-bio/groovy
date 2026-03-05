param(
    [Parameter(Mandatory = $true)]
    [string]$Owner,
    [Parameter(Mandatory = $true)]
    [string]$Repo,
    [string]$OutDir = ".\download"
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$headers = @{
    "User-Agent" = "OlehGroovyEditorDownloader"
    "Accept" = "application/vnd.github+json"
}
$assetName = "OlehGroovyEditor-windows-portable.zip"

New-Item -ItemType Directory -Path $OutDir -Force | Out-Null

$release = $null
$latestApi = "https://api.github.com/repos/$Owner/$Repo/releases/latest"
Write-Host "Fetching latest stable release from $latestApi"
try {
    $release = Invoke-RestMethod -Uri $latestApi -Headers $headers
} catch {
    Write-Host "Latest stable release not found. Falling back to full release list (includes beta)."
}

if (-not $release) {
    $listApi = "https://api.github.com/repos/$Owner/$Repo/releases?per_page=30"
    try {
        $releases = Invoke-RestMethod -Uri $listApi -Headers $headers
    } catch {
        throw "Cannot list releases for $Owner/$Repo without login. Make the repo public and ensure at least one release exists."
    }
    $release = $releases |
        Where-Object { $_.assets -and ($_.assets.name -contains $assetName) } |
        Sort-Object { [datetime]$_.published_at } -Descending |
        Select-Object -First 1
}

if (-not $release) {
    throw "No release with asset $assetName found for $Owner/$Repo."
}

$asset = $release.assets | Where-Object { $_.name -eq $assetName } | Select-Object -First 1
if (-not $asset) {
    throw "Asset $assetName not found in release $($release.tag_name)."
}

Write-Host "Using release: $($release.tag_name)"
$zipPath = Join-Path $OutDir $asset.name
Write-Host "Downloading $($asset.browser_download_url)"
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zipPath -Headers $headers

$extractDir = Join-Path $OutDir "OlehGroovyEditor"
if (Test-Path $extractDir) {
    Remove-Item $extractDir -Recurse -Force
}
Expand-Archive -Path $zipPath -DestinationPath $extractDir -Force

$exePath = Join-Path $extractDir "oleh-groovy-editor.exe"
$batPath = Join-Path $extractDir "OlehGroovyEditor.bat"
if (-not (Test-Path $exePath)) {
    throw "Downloaded zip is missing oleh-groovy-editor.exe"
}
if (-not (Test-Path $batPath)) {
    throw "Downloaded zip is missing OlehGroovyEditor.bat"
}

Write-Host "Downloaded: $zipPath"
Write-Host "Extracted to: $extractDir"
Write-Host "Run: $batPath"
