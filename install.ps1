# claudibeat in one command:
#   irm https://raw.githubusercontent.com/PixelShino/claudibeat/main/install.ps1 | iex
# Downloads the latest release into %LOCALAPPDATA%\Programs\claudibeat, then
# runs `claudibeat --setup`: the status-band mod for Claude Desktop, starting
# with Windows, and the strip itself.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repo = 'PixelShino/claudibeat'
$dir = Join-Path $env:LOCALAPPDATA 'Programs\claudibeat'
$exe = Join-Path $dir 'claudibeat.exe'

$release = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest" -Headers @{ 'User-Agent' = 'claudibeat-install' }
$asset = $release.assets | Where-Object name -eq 'claudibeat.exe' | Select-Object -First 1
$sum = $release.assets | Where-Object name -eq 'claudibeat.exe.sha256' | Select-Object -First 1
if (-not $asset) { throw "No claudibeat.exe in the latest release ($($release.tag_name))." }

$tmp = Join-Path $env:TEMP "claudibeat-$($release.tag_name).exe"
Write-Host "Downloading claudibeat $($release.tag_name)..."
Invoke-WebRequest $asset.browser_download_url -OutFile $tmp -UseBasicParsing
if ($sum) {
    $want = (Invoke-RestMethod $sum.browser_download_url -UseBasicParsing).ToString().Trim()
    $got = (Get-FileHash $tmp -Algorithm SHA256).Hash
    if ($want -ne $got) { Remove-Item $tmp; throw "Checksum mismatch: expected $want, got $got." }
}

# A running copy quits politely first; a forced kill makes Explorer refuse the new strip for minutes.
if (Test-Path $exe) {
    Start-Process $exe -ArgumentList '--quit' -Wait -ErrorAction SilentlyContinue
    for ($i = 0; $i -lt 20 -and (Get-Process claudibeat -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 500 }
}
New-Item -ItemType Directory -Force $dir | Out-Null
Move-Item $tmp $exe -Force

Start-Process $exe -ArgumentList '--setup'
Write-Host "claudibeat $($release.tag_name) is in $dir. Open a new Claude chat so the mod starts reporting."
