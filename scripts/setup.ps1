# Build monk from source, put it on your PATH, and wire up privileges.
# Windows (PowerShell 5+). Run from a clone of the repo:
#   .\scripts\setup.ps1
# For the privileged step, open the terminal *as Administrator*.
$ErrorActionPreference = 'Stop'

$Bin   = 'monk'
$Total = 4
$script:Step = 0

function Write-Step($msg) {
    $script:Step++
    Write-Host ""
    Write-Host "[$script:Step/$Total] " -ForegroundColor Blue -NoNewline
    Write-Host $msg -ForegroundColor White
}
function Write-Ok($msg)   { Write-Host "  + $msg" -ForegroundColor Green }
function Write-Info($msg) { Write-Host "  $msg" -ForegroundColor DarkGray }
function Write-Warn($msg) { Write-Host "  ! $msg" -ForegroundColor Yellow }
function Fail($msg)       { Write-Host ""; Write-Host "error: $msg" -ForegroundColor Red; exit 1 }

function Ask($msg) {
    $reply = Read-Host "  ? $msg [Y/n]"
    return ($reply -notmatch '^[nN]')
}

# ----- locate repo root ------------------------------------------------------
$RepoRoot = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path (Join-Path $RepoRoot 'Cargo.toml'))) {
    Fail "run this from a clone of monk-cli (Cargo.toml not found)"
}
Set-Location $RepoRoot

Write-Host " monk - build from source " -ForegroundColor Blue
Write-Info $RepoRoot

# ----- 1. Rust toolchain -----------------------------------------------------
Write-Step "Checking the Rust toolchain"
if (Get-Command cargo -ErrorAction SilentlyContinue) {
    $ver = (cargo --version).Split(' ')[1]
    Write-Ok "cargo $ver found"
} else {
    Write-Warn "Rust toolchain not found."
    if (Ask "Install it now via rustup?") {
        Write-Info "downloading rustup-init..."
        $init = Join-Path $env:TEMP 'rustup-init.exe'
        $arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'aarch64' } else { 'x86_64' }
        Invoke-WebRequest -Uri "https://static.rust-lang.org/rustup/dist/$arch-pc-windows-msvc/rustup-init.exe" -OutFile $init
        & $init -y
        $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
        Write-Ok "Rust installed"
    } else {
        Fail "Rust 1.82+ is required - see https://rustup.rs"
    }
}

# ----- 2. Build & install ----------------------------------------------------
# `cargo install` compiles the release binary and copies it into ~\.cargo\bin,
# so there is no separate `cargo build` step (that would just compile twice).
Write-Step "Building and installing $Bin (this can take a few minutes)"
cargo install --path . --force
if ($LASTEXITCODE -ne 0) { Fail "cargo install failed" }
$CargoBin  = if ($env:CARGO_HOME) { Join-Path $env:CARGO_HOME 'bin' } else { Join-Path $env:USERPROFILE '.cargo\bin' }
$Installed = Join-Path $CargoBin "$Bin.exe"
Write-Ok "installed $Installed"

# ----- 3. Ensure it is on PATH -----------------------------------------------
Write-Step "Making sure $CargoBin is on your PATH"
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -contains $CargoBin) {
    Write-Ok "$CargoBin is already on your PATH"
} else {
    Write-Warn "$CargoBin is not on your user PATH"
    if (Ask "Add it to your user PATH now?") {
        $newPath = if ($userPath) { "$userPath;$CargoBin" } else { $CargoBin }
        [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
        $env:Path = "$env:Path;$CargoBin"
        Write-Ok "added - open a new terminal for it to take effect everywhere"
    } else {
        Write-Info "add this folder to PATH yourself: $CargoBin"
    }
}

# ----- 4. Privileges / service ----------------------------------------------
Write-Step "Wiring up privileges (monk setup)"
Write-Info "Windows: installs a logon scheduled task that needs elevation."
$isAdmin = ([Security.Principal.WindowsPrincipal] `
    [Security.Principal.WindowsIdentity]::GetCurrent()
).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Warn "this terminal is not elevated - 'monk setup' may fail."
    Write-Info "re-open PowerShell as Administrator if it does."
}
if (Ask "Run '$Bin setup' now?") {
    & $Installed setup
    if ($LASTEXITCODE -ne 0) { Write-Warn "'$Bin setup' did not finish - run it later with: $Bin setup" }
} else {
    Write-Info "skipped - run it later with: $Bin setup"
}

Write-Host ""
Write-Host "+ Done. " -ForegroundColor Green -NoNewline
Write-Host "Try: monk --help"
