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
#   .\scripts\build-pgo.ps1                  # full cycle (interactive GUI training)
#   .\scripts\build-pgo.ps1 -NonInteractive  # headless boot-training (no operator)
#   .\scripts\build-pgo.ps1 -SkipTrain       # rebuild with existing profile
#   .\scripts\build-pgo.ps1 -CleanFirst      # wipe old profile data
#
# -NonInteractive (v1.7.206):
#   Replaces the operator-driven GUI run + ENTER prompt with an automated
#   boot-training pass: it launches the instrumented APP binary (NOT a test
#   harness — see note below), lets it run for -BootTrainSeconds so the boot
#   hot paths execute (DB open + migrations, model-catalog load, pre-loop
#   semantic recall, embeddings init, prompt scaffolding), then closes it
#   GRACEFULLY (WM_CLOSE, never /F) so profile-generate's atexit handler
#   flushes the .profraw. No window interaction required → CI-friendly.
#
#   This is a BOOT profile, lighter than a live interactive session driving
#   streaming + tool loops. For the richest profile, prefer the interactive
#   mode with a real workload when a human is at the keyboard.
#
#   WHY the app binary and not `cargo test`: on this crate
#   (crate-type = staticlib + cdylib + rlib) the instrumented *unit-test*
#   harness fails to load on Windows MSVC with STATUS_ENTRYPOINT_NOT_FOUND
#   (0xc0000139) — the test exe resolves the colliding lucy_svelte_lib.dll
#   with mismatched instrumented exports. The real app binary links cleanly,
#   so we train on it instead.
#
# Prerequisites:
#   - rustup component add llvm-tools-preview
#   - The directory you run this from must be the project root.
#   - For -NonInteractive: an interactive desktop session, and no other Lucy
#     instance running (the single-instance plugin would focus-and-exit the
#     training launch, producing no profile).

[CmdletBinding()]
param(
    [switch]$CleanFirst,
    [switch]$SkipTrain,
    [switch]$NonInteractive,
    [int]$BootTrainSeconds = 45,
    [string]$ProfilesDir = ".\target\pgo-profiles"
)

$ErrorActionPreference = "Stop"
$ProgressPreference    = "SilentlyContinue"

# Resolve absolute paths up front — RUSTFLAGS needs them.
# NOTE: Windows PowerShell 5.1-compatible (no `??` / `?.` — those are pwsh 7+
# only, and this box may not have pwsh installed). v1.7.206.
$resolved = Resolve-Path -LiteralPath $ProfilesDir -ErrorAction SilentlyContinue
if ($resolved) {
    $ProfilesDir = $resolved.Path
} else {
    $ProfilesDir = (New-Item -ItemType Directory -Force -Path $ProfilesDir).FullName
}

$MergedProfile = Join-Path $ProfilesDir "merged.profdata"

Write-Host "── Lucy PGO Build ──" -ForegroundColor Cyan
Write-Host "Profiles dir : $ProfilesDir"
Write-Host "Merged file  : $MergedProfile"
Write-Host ""

# ── Sanity: llvm-tools-preview installed? ──────────────────────────────────
$llvmCmd = Get-Command llvm-profdata.exe -ErrorAction SilentlyContinue
$llvmProfdata = if ($llvmCmd) { $llvmCmd.Source } else { $null }
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

# ── Frontend bundle (required for a RUNNABLE binary) ────────────────────────
# Tauri embeds `frontendDist` (../build) at COMPILE time, but the binary only
# LOADS those embedded assets when built with the `custom-protocol` feature.
# Per tauri's build.rs: `let dev = !custom_protocol;`. Without it the app runs
# in DEV mode and navigates to `devUrl` (http://localhost:1420) — which isn't
# running for a standalone launch, so you get ERR_CONNECTION_REFUSED. The
# `tauri build` CLI enables this feature for you; a raw `cargo build` does not.
# So we (a) build the frontend now and (b) pass `--features tauri/custom-protocol`
# to every cargo build below — making BOTH the instrumented training binary and
# the final optimized binary real, runnable, prod-mode Lucy (and the training
# then also exercises the frontend-init + invoke-handler hot paths).
Write-Host "[*] Building frontend bundle (npm run build)..." -ForegroundColor Cyan
npm run build
if ($LASTEXITCODE -ne 0) { throw "frontend build (npm run build) failed" }
Write-Host ""

if (-not $SkipTrain) {
    if ($NonInteractive) {
        # ── Phase 1 (headless): build the instrumented APP binary ──────────
        Write-Host "[1/4] Building instrumented app binary (release-pgo-gen)..." -ForegroundColor Cyan
        $env:RUSTFLAGS = "-Cprofile-generate=$ProfilesDir"
        $env:LLVM_PROFILE_FILE = (Join-Path $ProfilesDir "lucy-%p-%m.profraw")
        Push-Location src-tauri
        try {
            cargo build --profile release-pgo-gen --features tauri/custom-protocol
            if ($LASTEXITCODE -ne 0) { throw "instrumented app build failed" }
        } finally { Pop-Location }

        # ── Phase 2 (headless): boot-train, then close gracefully ──────────
        # Launch the real instrumented binary, let the boot hot paths run,
        # then send WM_CLOSE (CloseMainWindow) so profile-generate's atexit
        # handler flushes .profraw. We do NOT /F-kill on the happy path — a
        # forced TerminateProcess skips atexit and would drop the profile.
        Write-Host "[2/4] Headless boot-training for $BootTrainSeconds s..." -ForegroundColor Cyan
        $binary = ".\src-tauri\target\release-pgo-gen\lucy-svelte.exe"
        if (-not (Test-Path $binary)) {
            Write-Host "ERROR: instrumented binary not at $binary" -ForegroundColor Red
            exit 1
        }
        $proc = Start-Process -FilePath $binary -WorkingDirectory (Get-Location) -PassThru
        Start-Sleep -Seconds $BootTrainSeconds
        if (-not $proc.HasExited) {
            Write-Host "  Boot window settled — sending graceful close (WM_CLOSE)..."
            $null = $proc.CloseMainWindow()
            for ($w = 0; $w -lt 10 -and -not $proc.HasExited; $w++) { Start-Sleep -Seconds 1 }
            if (-not $proc.HasExited) {
                Write-Host "  Did not exit on WM_CLOSE — force-killing (profile may be partial)." -ForegroundColor Yellow
                $proc | Stop-Process -Force
            }
        } else {
            Write-Host "  WARNING: instrumented Lucy exited before training window — another" -ForegroundColor Yellow
            Write-Host "           instance may have absorbed it (single-instance plugin)." -ForegroundColor Yellow
        }
        Remove-Item Env:\RUSTFLAGS
        Remove-Item Env:\LLVM_PROFILE_FILE
    } else {
        # ── Phase 1: instrumented build ────────────────────────────────────
        Write-Host "[1/4] Building instrumented binary (release-pgo-gen)..." -ForegroundColor Cyan
        $env:RUSTFLAGS = "-Cprofile-generate=$ProfilesDir"
        Push-Location src-tauri
        try {
            cargo build --profile release-pgo-gen --features tauri/custom-protocol
            if ($LASTEXITCODE -ne 0) { throw "instrumented build failed" }
        } finally { Pop-Location }
        Remove-Item Env:\RUSTFLAGS

        # ── Phase 2: training run ──────────────────────────────────────────
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
    }

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

    # Coverage sanity: surface how much the profile actually captured. A merged
    # profile with a near-zero "Total count" means training exercised almost no
    # real code (e.g. the binary exited immediately) — the optimized build would
    # then be PGO in name only. We print the summary so a hollow profile is
    # visible rather than silently shipped.
    Write-Host "  Profile coverage summary:"
    $summary = & $llvmProfdata show $MergedProfile 2>$null |
        Select-String -Pattern 'Total functions|Maximum function count|Maximum internal block count|Total count'
    if ($summary) { $summary | ForEach-Object { Write-Host "    $($_.Line.Trim())" } }
    else { Write-Host "    (llvm-profdata show produced no summary)" -ForegroundColor Yellow }
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
    cargo build --profile release-pgo-use --features tauri/custom-protocol
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
