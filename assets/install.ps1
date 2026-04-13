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
    $latest = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
    $version = $latest.tag_name
}

$url  = "https://github.com/$Repo/releases/download/$version/$Bin-$version-$target.zip"
$tmp  = New-Item -ItemType Directory -Path ([IO.Path]::Combine([IO.Path]::GetTempPath(), [Guid]::NewGuid()))
$zip  = Join-Path $tmp "monk.zip"

$archive = "$Bin-$version-$target.zip"
$sumsUrl = "https://github.com/$Repo/releases/download/$version/SHA256SUMS.txt"
$sumsFile = Join-Path $tmp "SHA256SUMS.txt"

Write-Host "==> downloading $url"
Invoke-WebRequest -Uri $url -OutFile $zip
Invoke-WebRequest -Uri $sumsUrl -OutFile $sumsFile

Write-Host "==> verifying checksum"
$expected = (Get-Content $sumsFile | Where-Object { $_.Contains($archive) }) -replace '\s+.*$',''
$actual = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
if ($actual -ne $expected) { throw "checksum mismatch: expected $expected, got $actual" }

Expand-Archive -Path $zip -DestinationPath $tmp -Force

New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
Copy-Item -Path (Join-Path $tmp "$Bin-$version-$target" "$Bin.exe") -Destination (Join-Path $InstallDir "$Bin.exe") -Force
Remove-Item -Recurse -Force $tmp

Write-Host "==> installed $Bin $version to $InstallDir\$Bin.exe"
if (($env:Path -split ';') -notcontains $InstallDir) {
    Write-Host "add $InstallDir to your PATH"
}
