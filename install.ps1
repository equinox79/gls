# Install the latest gls release (Windows x64).
#   irm https://raw.githubusercontent.com/equinox79/gls/main/install.ps1 | iex
# Options (environment variables):
#   GLS_VERSION         version to install, for example 0.2.0 (default: the latest release)
#   GLS_INSTALL_DIR     where to put gls.exe (default: %LOCALAPPDATA%\Programs\gls)
#   GLS_NO_MODIFY_PATH  set to 1 to leave your PATH alone
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$repo = 'equinox79/gls'
$target = 'x86_64-pc-windows-msvc'
$dir = if ($env:GLS_INSTALL_DIR) { $env:GLS_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\gls' }

if (-not [Environment]::Is64BitOperatingSystem) { throw 'gls has a prebuilt binary for 64-bit Windows only.' }

$version = if ($env:GLS_VERSION) { $env:GLS_VERSION.TrimStart('v') } else { '' }
if (-not $version) {
    # /releases/latest redirects to /releases/tag/vX.Y.Z (no API, no rate limit)
    $r = Invoke-WebRequest -UseBasicParsing "https://github.com/$repo/releases/latest"
    $final = if ($r.BaseResponse.RequestMessage) { $r.BaseResponse.RequestMessage.RequestUri.AbsoluteUri } else { $r.BaseResponse.ResponseUri.AbsoluteUri }
    $version = ($final -split '/v')[-1]
}
if (-not $version) { throw 'Could not work out the version.' }

$name = "gls-v$version-$target"
$base = "https://github.com/$repo/releases/download/v$version"
$tmp = Join-Path ([IO.Path]::GetTempPath()) ('gls-install-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    Write-Host "Installing gls $version ($target) to $dir"
    Invoke-WebRequest -UseBasicParsing "$base/$name.zip" -OutFile "$tmp\$name.zip"
    Invoke-WebRequest -UseBasicParsing "$base/SHA256SUMS" -OutFile "$tmp\SHA256SUMS"

    $line = Get-Content "$tmp\SHA256SUMS" | Where-Object { $_ -match ('\s' + [regex]::Escape($name) + '\.zip$') } | Select-Object -First 1
    if (-not $line) { throw "$name.zip is not in SHA256SUMS" }
    $want = ($line -split '\s+')[0].ToLower()
    $got = (Get-FileHash "$tmp\$name.zip" -Algorithm SHA256).Hash.ToLower()
    if ($want -ne $got) { throw "Checksum mismatch (expected $want, got $got)" }

    Expand-Archive "$tmp\$name.zip" -DestinationPath $tmp -Force
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    Copy-Item "$tmp\$name\gls.exe" (Join-Path $dir 'gls.exe') -Force
}
finally {
    try { [IO.Directory]::Delete($tmp, $true) } catch { }
}

Write-Host "Installed: $(Join-Path $dir 'gls.exe')"
& (Join-Path $dir 'gls.exe') --version

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$onPath = ($userPath -split ';') -contains $dir
if (-not $onPath -and $env:GLS_NO_MODIFY_PATH -ne '1') {
    [Environment]::SetEnvironmentVariable('Path', (($userPath.TrimEnd(';') + ';' + $dir).TrimStart(';')), 'User')
    Write-Host "Added $dir to your user PATH. Open a new terminal to use gls."
}
