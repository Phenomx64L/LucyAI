# ── build-pgo.ps1 — Profile-Guided Optimization pipeline (v1.7.86) ─────────
#
# Builds Lucy with PGO in four phases:
#
#   Phase 1 — INSTRUMENT
#     Builds the binary with `-C profile-generate=PROFILES_DIR`. The
#     resulting Lucy is ~10-20 % slower and writes raw profile data
#     to PROFILES_DIR every time a hot function is called.
#
#   Phase 2 — TRAIN
#     Runs Lucy under a representative workload so the profile data
#     captures what real users exercise: memory recall, auto-routing,
#     skill picker, streaming, prompt build, SIMD cosine.
#     The script launches Lucy and waits for the operator to press
#     ENTER after a few minutes of normal use. The longer the training
#     run, the better the resulting profile — 5-10 minutes of real
#     interactions is plenty.
#
#   Phase 3 — MERGE
#     Runs `llvm-profdata merge` to collapse the raw `.profraw` files
#     into a single `merged.profdata` file the optimized compile reads.
#
#   Phase 4 — OPTIMIZE
#     Re-builds with `-C profile-use=merged.profdata`. The compiler
#     uses the training data to make smarter decisions: inlines
#     functions on hot paths, lays out branches so the predictor stays
#     warm, allocates registers around the actual call frequencies.
#
# Expected gain: 5-15 % on hot paths. The bigger wins are on the SIMD
# cosine loop and the prompt-section assembly — both touched every
# turn but rarely benchmarked.
#
# Usage:
#   .\scripts\build-pgo.ps1               # full cycle
#   .\scripts\build-pgo.ps1 -SkipTrain    # rebuild with existing profile
#   .\scripts\build-pgo.ps1 -CleanFirst   # wipe old profile data
#
# Prerequisites:
#   - rustup component add llvm-tools-preview
#   - The directory you run this from must be the project root.

[CmdletBinding()]
param(
    [switch]$CleanFirst,
    [switch]$SkipTrain,
    [string]$ProfilesDir = ".\target\pgo-profiles"
)

$ErrorActionPreference = "Stop"
$ProgressPreference    = "SilentlyContinue"

# Resolve absolute paths up front — RUSTFLAGS needs them.
$ProfilesDir = (Resolve-Path -LiteralPath $ProfilesDir -ErrorAction SilentlyContinue) `
    ?? (New-Item -ItemType Directory -Force -Path $ProfilesDir).FullName

$MergedProfile = Join-Path $ProfilesDir "merged.profdata"

Write-Host "── Lucy PGO Build ──" -ForegroundColor Cyan
Write-Host "Profiles dir : $ProfilesDir"
Write-Host "Merged file  : $MergedProfile"
Write-Host ""

# ── Sanity: llvm-tools-preview installed? ──────────────────────────────────
$llvmProfdata = (Get-Command llvm-profdata.exe -ErrorAction SilentlyContinue)?.Source
if (-not $llvmProfdata) {
    # Try the rustup-managed copy.
    $sysroot = (& rustc --print sysroot).Trim()
    $candidate = Join-Path $sysroot "lib\rustlib\x86_64-pc-windows-msvc\bin\llvm-profdata.exe"
    if (Test-Path $candidate) { $llvmProfdata = $candidate }
}
if (-not $llvmProfdata) {
    Write-Host "ERROR: llvm-profdata.exe not found." -ForegroundColor Red
    Write-Host "Install via:  rustup component add llvm-tools-preview" -ForegroundColor Yellow
    exit 1
}
Write-Host "llvm-profdata: $llvmProfdata"
Write-Host ""

# ── Phase 0: optional clean ────────────────────────────────────────────────
if ($CleanFirst) {
    Write-Host "[0/4] Cleaning old profile data..." -ForegroundColor Yellow
    Remove-Item -Recurse -Force $ProfilesDir -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path $ProfilesDir | Out-Null
}

if (-not $SkipTrain) {
    # ── Phase 1: instrumented build ────────────────────────────────────────
    Write-Host "[1/4] Building instrumented binary (release-pgo-gen)..." -ForegroundColor Cyan
    $env:RUSTFLAGS = "-Cprofile-generate=$ProfilesDir"
    Push-Location src-tauri
    try {
        cargo build --profile release-pgo-gen
        if ($LASTEXITCODE -ne 0) { throw "instrumented build failed" }
    } finally { Pop-Location }
    Remove-Item Env:\RUSTFLAGS

    # ── Phase 2: training run ──────────────────────────────────────────────
    Write-Host ""
    Write-Host "[2/4] Training run." -ForegroundColor Cyan
    Write-Host "  - Lucy will launch now."
    Write-Host "  - Use her for 5-10 minutes of representative work:"
    Write-Host "      * open Memory Browser, run a few recall queries"
    Write-Host "      * open the Memory Graph, drag a couple of nodes"
    Write-Host "      * send 3-5 prompts of different sizes"
    Write-Host "      * trigger an auto-route (security keyword)"
    Write-Host "      * run /diagnostico"
    Write-Host "  - Close Lucy normally when done."
    Write-Host "  - Then press ENTER here to continue."
    Write-Host ""

    $binary = ".\src-tauri\target\release-pgo-gen\lucy-svelte.exe"
    if (-not (Test-Path $binary)) {
        Write-Host "ERROR: instrumented binary not at $binary" -ForegroundColor Red
        exit 1
    }
    Start-Process -FilePath $binary -WorkingDirectory (Get-Location)
    Read-Host "Press ENTER after the training session is complete"

    # ── Phase 3: merge profiles ────────────────────────────────────────────
    Write-Host ""
    Write-Host "[3/4] Merging profile data..." -ForegroundColor Cyan
    $rawFiles = Get-ChildItem -Path $ProfilesDir -Filter "*.profraw" -ErrorAction SilentlyContinue
    if ($rawFiles.Count -eq 0) {
        Write-Host "ERROR: no .profraw files in $ProfilesDir" -ForegroundColor Red
        Write-Host "  Did the instrumented Lucy run and exit cleanly?" -ForegroundColor Yellow
        exit 1
    }
    Write-Host "  Found $($rawFiles.Count) raw profile file(s)"
    & $llvmProfdata merge -o $MergedProfile $ProfilesDir\*.profraw
    if ($LASTEXITCODE -ne 0) { throw "profdata merge failed" }
    Write-Host "  Merged → $MergedProfile"
} else {
    Write-Host "[1-3/4] SKIPPED via -SkipTrain — reusing existing $MergedProfile" -ForegroundColor Yellow
    if (-not (Test-Path $MergedProfile)) {
        Write-Host "ERROR: no merged profile found and -SkipTrain was passed." -ForegroundColor Red
        exit 1
    }
}

# ── Phase 4: optimized build ───────────────────────────────────────────────
Write-Host ""
Write-Host "[4/4] Building optimized binary (release-pgo-use)..." -ForegroundColor Cyan
$env:RUSTFLAGS = "-Cprofile-use=$MergedProfile -Cllvm-args=-pgo-warn-missing-function"
Push-Location src-tauri
try {
    cargo build --profile release-pgo-use
    if ($LASTEXITCODE -ne 0) { throw "optimized build failed" }
} finally { Pop-Location }
Remove-Item Env:\RUSTFLAGS

$optimized = ".\src-tauri\target\release-pgo-use\lucy-svelte.exe"
if (Test-Path $optimized) {
    $size = (Get-Item $optimized).Length / 1MB
    Write-Host ""
    Write-Host "✓ PGO build complete." -ForegroundColor Green
    Write-Host "  Binary: $optimized ($([math]::Round($size, 2)) MB)"
    Write-Host "  Compare against the standard release to measure your gain."
} else {
    Write-Host "WARNING: optimized binary not found at $optimized" -ForegroundColor Yellow
}
