# Writes the three winget manifests for one release of claudibeat.
# The release workflow runs it; the result goes to microsoft/winget-pkgs as
# manifests/p/PixelShino/claudibeat/<version>/.
param(
    [Parameter(Mandatory)] [string] $Version,
    [Parameter(Mandatory)] [string] $Sha256,
    [Parameter(Mandatory)] [string] $Out
)

$id = 'PixelShino.claudibeat'
$url = "https://github.com/PixelShino/claudibeat/releases/download/v$Version/claudibeat.exe"
$schema = '1.6.0'
$dir = Join-Path $Out "manifests/p/PixelShino/claudibeat/$Version"
New-Item -ItemType Directory -Force $dir | Out-Null

$files = @{
    "$id.yaml" = @"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.version.$schema.schema.json
PackageIdentifier: $id
PackageVersion: $Version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: $schema
"@
    "$id.installer.yaml" = @"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.installer.$schema.schema.json
PackageIdentifier: $id
PackageVersion: $Version
MinimumOSVersion: 10.0.22000.0
InstallerType: portable
Commands:
  - claudibeat
Installers:
  - Architecture: x64
    InstallerUrl: $url
    InstallerSha256: $Sha256
ManifestType: installer
ManifestVersion: $schema
"@
    "$id.locale.en-US.yaml" = @"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.defaultLocale.$schema.schema.json
PackageIdentifier: $id
PackageVersion: $Version
PackageLocale: en-US
Publisher: Dmitry Goldobin
PublisherUrl: https://github.com/PixelShino
PackageName: claudibeat
PackageUrl: https://github.com/PixelShino/claudibeat
License: MIT
LicenseUrl: https://github.com/PixelShino/claudibeat/blob/main/LICENSE
ShortDescription: Claude Desktop sessions, memory and plan limit in the left corner of the Windows 11 taskbar.
Description: Shows which Claude Desktop sessions wait for you, which have finished and which still work, right in the Windows 11 taskbar, with memory use and the share of your plan limit spent. Run claudibeat --setup once to install its Claude Desktop mod and start it with Windows.
Tags:
  - claude
  - claude-desktop
  - taskbar
  - widget
ReleaseNotesUrl: https://github.com/PixelShino/claudibeat/releases/tag/v$Version
ManifestType: defaultLocale
ManifestVersion: $schema
"@
}

foreach ($name in $files.Keys) {
    # winget-pkgs wants UTF-8 without a BOM and LF line ends.
    $text = $files[$name].Replace("`r`n", "`n") + "`n"
    [IO.File]::WriteAllText((Join-Path $dir $name), $text, [Text.UTF8Encoding]::new($false))
}
Write-Host "winget manifests for $Version in $dir"
