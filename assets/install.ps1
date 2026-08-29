$ErrorActionPreference = 'Stop'

$Repo = 'mdportnov/monk-cli'
$Bin  = 'monk'
$InstallDir = if ($env:MONK_INSTALL_DIR) { $env:MONK_INSTALL_DIR } else { "$env:LOCALAPPDATA\monk\bin" }

$arch = switch ($env:PROCESSOR_ARCHITECTURE) {
    'AMD64' { 'x86_64' }
    'ARM64' { 'aarch64' }
    default { throw "unsupported arch: $($env:PROCESSOR_ARCHITECTURE)" }
}
$target = "$arch-pc-windows-msvc"

$version = $env:MONK_VERSION
if (-not $version) {
    try {
        $latest = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
    } catch {
        throw "could not resolve latest version (github api rate limit?) - set MONK_VERSION=vX.Y.Z"
    }
    $version = $latest.tag_name
}
# Release tags and asset names carry the `v`; accept MONK_VERSION with or
# without it instead of building a 404 URL.
if ($version -notlike 'v*') { $version = "v$version" }

$archive  = "$Bin-$version-$target.zip"
$url      = "https://github.com/$Repo/releases/download/$version/$archive"
$sumsUrl  = "https://github.com/$Repo/releases/download/$version/SHA256SUMS.txt"
$tmp      = New-Item -ItemType Directory -Path ([IO.Path]::Combine([IO.Path]::GetTempPath(), [Guid]::NewGuid()))

try {
    $zip      = Join-Path $tmp "monk.zip"
    $sumsFile = Join-Path $tmp "SHA256SUMS.txt"

    Write-Host "==> downloading $url"
    Invoke-WebRequest -Uri $url -OutFile $zip
    Invoke-WebRequest -Uri $sumsUrl -OutFile $sumsFile

    Write-Host "==> verifying checksum"
    $line = Get-Content $sumsFile | Where-Object { $_.Contains($archive) } | Select-Object -First 1
    if (-not $line) { throw "no checksum for $archive in SHA256SUMS.txt" }
    $expected = ($line -split '\s+')[0].ToLower()
    $actual   = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
    if ($actual -ne $expected) { throw "checksum mismatch: expected $expected, got $actual" }

    # Clear the mark-of-the-web before extracting, so SmartScreen does not
    # block the extracted monk.exe (the macOS script strips the quarantine
    # xattr for the same reason).
    Unblock-File -Path $zip

    Expand-Archive -Path $zip -DestinationPath $tmp -Force

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    $exe = Join-Path $InstallDir "$Bin.exe"
    Copy-Item -Path (Join-Path $tmp "$Bin-$version-$target" "$Bin.exe") -Destination $exe -Force
    Unblock-File -Path $exe
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

Write-Host "==> installed $Bin $version to $InstallDir\$Bin.exe"

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $InstallDir) {
    $cmd = '$p = [Environment]::GetEnvironmentVariable(''Path'', ''User''); ' +
           '[Environment]::SetEnvironmentVariable(''Path'', $p + '';' + $InstallDir + ''', ''User'')'
    Write-Host "add $InstallDir to your PATH, then open a new terminal:"
    Write-Host "  $cmd"
}

$isAdmin = ([Security.Principal.WindowsPrincipal] `
    [Security.Principal.WindowsIdentity]::GetCurrent()
).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

# An installed binary alone blocks nothing until the logon task is wired up,
# so finish the job here. MONK_SETUP=1 forces it (unattended), MONK_SETUP=0
# skips it.
function Invoke-Setup {
    if (-not $isAdmin) {
        Write-Host "note: this terminal is not elevated - the logon task needs Administrator."
        Write-Host "      everything else (config, completions) still gets set up."
    }
    & $exe setup
    if ($LASTEXITCODE -ne 0) {
        Write-Host "warning: '$Bin setup' did not finish - run it again later" -ForegroundColor Yellow
    }
}

if ($env:MONK_SETUP -eq '0') {
    Write-Host "next: run '$Bin setup' from a terminal opened as Administrator."
} elseif ($env:MONK_SETUP -eq '1') {
    Invoke-Setup
} elseif ([Environment]::UserInteractive) {
    $reply = Read-Host "==> run '$Bin setup' now? [Y/n]"
    if ($reply -notmatch '^[nN]') { Invoke-Setup }
    else { Write-Host "next: run '$Bin setup' from a terminal opened as Administrator." }
} else {
    Write-Host "next: run '$Bin setup' from a terminal opened as Administrator."
}
