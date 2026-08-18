// ── simd_cosine.rs — SIMD-accelerated cosine similarity (v1.7.19) ────────
//
// Lucy's skills auto-routing Tier 2 computes cosine similarity between a
// query embedding and 213 cached SKILL.md embeddings (768-dim, f32) per
// turn. With the previous scalar loop this took ~200ms on the i9-11950H.
// That's ~1% of an interactive turn, but it's the ONLY local CPU hot
// path that runs synchronously on every prompt.
//
// This module replaces the three scattered scalar implementations
// (embeddings.rs, memory.rs, vec_index.rs) with a single dispatched
// entry point that picks the best available instruction set at boot:
//
//   AVX-512F  (Tiger Lake-H, Zen 4+)  — 16 f32 ops/cycle, 3-4× scalar
//   AVX2+FMA  (Haswell+, Zen 1+)       — 8 f32 ops/cycle, 2× scalar
//   scalar    (anything else, ARM, …)  — auto-vec by LLVM
//
// ── Why `#[target_feature]` instead of `RUSTFLAGS=-C target-cpu=...` ────
//
// `target-cpu` would force the WHOLE binary to require AVX-512, breaking
// portability (one of the two key requirements). `target_feature` on
// specific `unsafe` functions tells the compiler "emit these instructions
// in this function regardless of profile flags." It works even with
// `opt-level = "z"` because we're not asking the optimizer to vectorize
// for us — we're hand-rolling the intrinsics.
//
// ── Why we cache the dispatch decision ─────────────────────────────────
//
// `is_x86_feature_detected!` is cheap (a `__get_cpuid` + bit test) but
// it's still a branch on every call. The cache is a `OnceLock<Backend>`
// resolved once at first use and read with relaxed atomics afterwards.
//
// ── Frequency downclock note (Intel Tiger Lake-H 11th gen) ─────────────
//
// AVX-512 instructions on Skylake-SP through Tiger Lake trigger a brief
// (~1ms) frequency drop — the "AVX-512 license." For our usage pattern
// this is fine: skills routing runs as a single burst of ~5ms and then
// the CPU has 2-5 seconds of LLM wait time to recover frequency before
// the next burst. On Zen 4 (Ryzen 7000+) there's no downclock at all —
// AVX-512 is double-pumped over 256-bit ports.

use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Avx512,
    Avx2,
    Scalar,
}

impl Backend {
    pub fn name(self) -> &'static str {
        match self {
            Backend::Avx512 => "avx512f",
            Backend::Avx2   => "avx2+fma",
            Backend::Scalar => "scalar",
        }
    }
}

static BACKEND: OnceLock<Backend> = OnceLock::new();

/// Resolve the best backend for this CPU. Cached after first call.
/// On non-x86 targets always returns `Scalar`.
pub fn backend() -> Backend {
    *BACKEND.get_or_init(detect_backend)
}

fn detect_backend() -> Backend {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        // We need F + DQ + VL to use 512-bit loads, masked tail and
        // reduce_add. F alone isn't enough for the tail path we use.
        if std::arch::is_x86_feature_detected!("avx512f")
            && std::arch::is_x86_feature_detected!("avx512dq")
            && std::arch::is_x86_feature_detected!("avx512vl")
        {
            return Backend::Avx512;
        }
        if std::arch::is_x86_feature_detected!("avx2")
            && std::arch::is_x86_feature_detected!("fma")
        {
            return Backend::Avx2;
        }
    }
    Backend::Scalar
}

// ── Public API ─────────────────────────────────────────────────────────

/// Cosine similarity in [-1.0, 1.0]. Returns 0.0 on length mismatch or
/// zero-magnitude inputs (avoids NaN). Drop-in replacement for the
/// scattered scalar implementations.
#[inline]
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (dot, na, nb) = sums(a, b);
    if na <= 0.0 || nb <= 0.0 { return 0.0; }
    (dot / (na.sqrt() * nb.sqrt())).clamp(-1.0, 1.0)
}

/// Returns (dot, |a|², |b|²) using the best backend available. Useful
/// when you need the raw sums for batched normalisation (e.g. ranking
/// N candidates against one query — you only need |query|² once).
#[inline]
pub fn sums(a: &[f32], b: &[f32]) -> (f32, f32, f32) {
    debug_assert_eq!(a.len(), b.len());
    match backend() {
        Backend::Avx512 => unsafe { sums_avx512(a, b) },
        Backend::Avx2   => unsafe { sums_avx2(a, b) },
        Backend::Scalar => sums_scalar(a, b),
    }
}

// ── Scalar (portable fallback) ─────────────────────────────────────────
//
// Plain loop; LLVM is allowed to auto-vectorize at opt-level 2/3 but
// won't with our profile's `opt-level = "z"`. That's intentional: this
// path runs only when neither AVX-512 nor AVX2 is available, which on
// x86 means a pre-Haswell CPU — already so slow that a few extra ms
// here is noise.

#[inline]
fn sums_scalar(a: &[f32], b: &[f32]) -> (f32, f32, f32) {
    let mut dot = 0.0_f32;
    let mut na  = 0.0_f32;
    let mut nb  = 0.0_f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na  += a[i] * a[i];
        nb  += b[i] * b[i];
    }
    (dot, na, nb)
}

// ── AVX2 + FMA (universal x86 baseline since 2013) ─────────────────────
//
// Processes 8 f32 per iteration via 256-bit YMM registers + fused
// multiply-add. The tail (when len % 8 != 0) falls back to scalar.
// All 213 SKILL.md embeddings are 768-dim, so len is always a multiple
// of 8 and the tail is dead code in the steady state.

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2,fma")]
unsafe fn sums_avx2(a: &[f32], b: &[f32]) -> (f32, f32, f32) {
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;

    let len = a.len();
    let chunks = len / 8;

    let mut dot = _mm256_setzero_ps();
    let mut na  = _mm256_setzero_ps();
    let mut nb  = _mm256_setzero_ps();

    let pa = a.as_ptr();
    let pb = b.as_ptr();
    for i in 0..chunks {
        let va = _mm256_loadu_ps(pa.add(i * 8));
        let vb = _mm256_loadu_ps(pb.add(i * 8));
        dot = _mm256_fmadd_ps(va, vb, dot);
        na  = _mm256_fmadd_ps(va, va, na);
        nb  = _mm256_fmadd_ps(vb, vb, nb);
    }

    let mut s_dot = horizontal_sum_avx2(dot);
    let mut s_na  = horizontal_sum_avx2(na);
    let mut s_nb  = horizontal_sum_avx2(nb);

    // Tail: scalar for any leftover lanes. Skipped entirely when
    // len % 8 == 0 (always true for 768-dim embeddings).
    let tail_start = chunks * 8;
    for i in tail_start..len {
        let av = *a.get_unchecked(i);
        let bv = *b.get_unchecked(i);
        s_dot += av * bv;
        s_na  += av * av;
        s_nb  += bv * bv;
    }
    (s_dot, s_na, s_nb)
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn horizontal_sum_avx2(v: std::arch::x86_64::__m256) -> f32 {
    use std::arch::x86_64::*;
    // Add high 128 to low 128 → 128-bit vector with 4 lanes.
    let lo = _mm256_castps256_ps128(v);
    let hi = _mm256_extractf128_ps(v, 1);
    let sum128 = _mm_add_ps(lo, hi);
    // hadd twice: 4 → 2 → 1.
    let sum = _mm_hadd_ps(sum128, sum128);
    let sum = _mm_hadd_ps(sum, sum);
    _mm_cvtss_f32(sum)
}

// ── AVX-512F (Tiger Lake-H 11th gen, Zen 4+) ──────────────────────────
//
// Processes 16 f32 per iteration via 512-bit ZMM registers + fused
// multiply-add. For 768-dim that's 48 iterations per call vs 96 for
// AVX2 vs 768 for scalar.
//
// The horizontal reduce uses _mm512_reduce_add_ps which the compiler
// lowers to an efficient tree of vextractf64x4 + vaddps + vextractf128
// + vhaddps. We let the compiler do the lowering rather than hand-
// rolling because it picks differently on Intel vs AMD.

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx512f")]
unsafe fn sums_avx512(a: &[f32], b: &[f32]) -> (f32, f32, f32) {
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;

    let len = a.len();
    let chunks = len / 16;

    let mut dot = _mm512_setzero_ps();
    let mut na  = _mm512_setzero_ps();
    let mut nb  = _mm512_setzero_ps();

    let pa = a.as_ptr();
    let pb = b.as_ptr();
    for i in 0..chunks {
        let va = _mm512_loadu_ps(pa.add(i * 16));
        let vb = _mm512_loadu_ps(pb.add(i * 16));
        dot = _mm512_fmadd_ps(va, vb, dot);
        na  = _mm512_fmadd_ps(va, va, na);
        nb  = _mm512_fmadd_ps(vb, vb, nb);
    }

    let mut s_dot = _mm512_reduce_add_ps(dot);
    let mut s_na  = _mm512_reduce_add_ps(na);
    let mut s_nb  = _mm512_reduce_add_ps(nb);

    // Tail (never hit for 768-dim embeddings — len is a multiple of 16).
    let tail_start = chunks * 16;
    for i in tail_start..len {
        let av = *a.get_unchecked(i);
        let bv = *b.get_unchecked(i);
        s_dot += av * bv;
        s_na  += av * av;
        s_nb  += bv * bv;
    }
    (s_dot, s_na, s_nb)
}

// ── Benchmark — measure backend throughput end-to-end ──────────────────
//
// Runs `iters` cosine similarities of `dim`-element vectors against
// each backend that the host CPU supports. Used by the `/bench-simd`
// slash command to show real-world throughput numbers. We pin to the
// same input across backends so the timing comparison is fair (no
// micro-arch-dependent branch prediction differences from random data
// re-generation between runs).

#[derive(serde::Serialize)]
pub struct BenchEntry {
    pub backend:    &'static str,
    pub available:  bool,
    pub ms_total:   f64,
    pub ms_per_op:  f64,
    pub ops_per_s:  f64,
    pub speedup_vs_scalar: f64,
}

#[derive(serde::Serialize)]
pub struct BenchReport {
    pub iters:   u32,
    pub dim:     u32,
    pub entries: Vec<BenchEntry>,
    pub host_backend: &'static str,
}

#[tauri::command]
pub async fn bench_cosine(iters: Option<u32>, dim: Option<u32>) -> Result<BenchReport, String> {
    use std::time::Instant;

    let iters = iters.unwrap_or(50_000).clamp(1_000, 500_000);
    let dim   = dim.unwrap_or(768).clamp(64, 4_096) as usize;

    // Deterministic random vectors (xorshift64). Same seed across runs.
    let make = |seed: u64| -> Vec<f32> {
        let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        (0..dim).map(|_| {
            s ^= s << 13; s ^= s >> 7; s ^= s << 17;
            ((s as f32) / (u64::MAX as f32)) * 2.0 - 1.0
        }).collect()
    };
    let a = make(7);
    let b = make(13);

    // Run each path on a worker thread so the awaited future doesn't
    // block the Tauri runtime. spawn_blocking returns the Vec.
    let report = tokio::task::spawn_blocking(move || -> BenchReport {
        let mut entries: Vec<BenchEntry> = Vec::with_capacity(3);

        // --- Scalar reference --------------------------------------------------
        let t0 = Instant::now();
        let mut sink = 0.0_f32;
        for _ in 0..iters {
            let (d, n1, n2) = sums_scalar(&a, &b);
            sink += d / (n1.sqrt() * n2.sqrt() + 1e-9);
        }
        std::hint::black_box(sink);
        let ms_scalar = t0.elapsed().as_secs_f64() * 1000.0;
        let ops_scalar = (iters as f64) / (ms_scalar / 1000.0);
        entries.push(BenchEntry {
            backend: "scalar", available: true,
            ms_total: ms_scalar,
            ms_per_op: ms_scalar / iters as f64,
            ops_per_s: ops_scalar,
            speedup_vs_scalar: 1.0,
        });

        // --- AVX2 + FMA --------------------------------------------------------
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            let has_avx2 = std::arch::is_x86_feature_detected!("avx2")
                        && std::arch::is_x86_feature_detected!("fma");
            if has_avx2 {
                let t0 = Instant::now();
                let mut sink = 0.0_f32;
                for _ in 0..iters {
                    let (d, n1, n2) = unsafe { sums_avx2(&a, &b) };
                    sink += d / (n1.sqrt() * n2.sqrt() + 1e-9);
                }
                std::hint::black_box(sink);
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                let ops = (iters as f64) / (ms / 1000.0);
                entries.push(BenchEntry {
                    backend: "avx2+fma", available: true,
                    ms_total: ms,
                    ms_per_op: ms / iters as f64,
                    ops_per_s: ops,
                    speedup_vs_scalar: ms_scalar / ms,
                });
            } else {
                entries.push(BenchEntry {
                    backend: "avx2+fma", available: false,
                    ms_total: 0.0, ms_per_op: 0.0, ops_per_s: 0.0,
                    speedup_vs_scalar: 0.0,
                });
            }
        }

        // --- AVX-512F ----------------------------------------------------------
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            let has_512 = std::arch::is_x86_feature_detected!("avx512f");
            if has_512 {
                let t0 = Instant::now();
                let mut sink = 0.0_f32;
                for _ in 0..iters {
                    let (d, n1, n2) = unsafe { sums_avx512(&a, &b) };
                    sink += d / (n1.sqrt() * n2.sqrt() + 1e-9);
                }
                std::hint::black_box(sink);
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                let ops = (iters as f64) / (ms / 1000.0);
                entries.push(BenchEntry {
                    backend: "avx512f", available: true,
                    ms_total: ms,
                    ms_per_op: ms / iters as f64,
                    ops_per_s: ops,
                    speedup_vs_scalar: ms_scalar / ms,
                });
            } else {
                entries.push(BenchEntry {
                    backend: "avx512f", available: false,
                    ms_total: 0.0, ms_per_op: 0.0, ops_per_s: 0.0,
                    speedup_vs_scalar: 0.0,
                });
            }
        }

        BenchReport {
            iters, dim: dim as u32, entries,
            host_backend: backend().name(),
        }
    }).await.map_err(|e| format!("bench task join error: {}", e))?;

    Ok(report)
}

// ── Tauri command — expose to frontend for /cpu slash command ──────────

#[derive(serde::Serialize)]
pub struct SimdInfo {
    pub backend:        &'static str,
    pub has_avx512f:    bool,
    pub has_avx512dq:   bool,
    pub has_avx512vl:   bool,
    pub has_avx2:       bool,
    pub has_fma:        bool,
    pub arch:           &'static str,
}

#[tauri::command]
pub fn simd_info() -> SimdInfo {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        SimdInfo {
            backend:      backend().name(),
            has_avx512f:  std::arch::is_x86_feature_detected!("avx512f"),
            has_avx512dq: std::arch::is_x86_feature_detected!("avx512dq"),
            has_avx512vl: std::arch::is_x86_feature_detected!("avx512vl"),
            has_avx2:     std::arch::is_x86_feature_detected!("avx2"),
            has_fma:      std::arch::is_x86_feature_detected!("fma"),
            arch:         if cfg!(target_arch = "x86_64") { "x86_64" } else { "x86" },
        }
    }
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    SimdInfo {
        backend: backend().name(),
        has_avx512f: false, has_avx512dq: false, has_avx512vl: false,
        has_avx2: false,    has_fma: false,
        arch: std::env::consts::ARCH,
    }
}

// ── Tests ──────────────────────────────────────────────────────────────
//
// Equivalence tests verify each backend produces identical results
// (within FP epsilon) to the scalar reference. They run on whatever
// CPU the test host has — if it lacks AVX-512 the avx512 test is
// skipped via `is_x86_feature_detected`.

#[cfg(test)]
mod tests {
    use super::*;

    fn rand_vec(n: usize, seed: u64) -> Vec<f32> {
        // xorshift64 — deterministic, no rand crate dep
        let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        (0..n).map(|_| {
            s ^= s << 13; s ^= s >> 7; s ^= s << 17;
            ((s as f32) / (u64::MAX as f32)) * 2.0 - 1.0
        }).collect()
    }

    fn approx_eq(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() < eps || ((a - b).abs() / a.abs().max(b.abs()).max(1e-6)) < eps
    }

    #[test]
    fn cosine_identical_is_one() {
        let v: Vec<f32> = (0..768).map(|i| i as f32 * 0.01).collect();
        let c = cosine(&v, &v);
        assert!(approx_eq(c, 1.0, 1e-4), "got {}", c);
    }

    #[test]
    fn cosine_orthogonal_is_zero() {
        let mut a = vec![0.0_f32; 768]; a[0] = 1.0;
        let mut b = vec![0.0_f32; 768]; b[1] = 1.0;
        let c = cosine(&a, &b);
        assert!(approx_eq(c, 0.0, 1e-6), "got {}", c);
    }

    #[test]
    fn cosine_zero_vectors_no_nan() {
        let a = vec![0.0_f32; 768];
        let b = vec![0.0_f32; 768];
        let c = cosine(&a, &b);
        assert_eq!(c, 0.0);
        assert!(!c.is_nan());
    }

    #[test]
    fn cosine_length_mismatch_returns_zero() {
        assert_eq!(cosine(&[1.0, 2.0], &[1.0, 2.0, 3.0]), 0.0);
        assert_eq!(cosine(&[], &[]), 0.0);
    }

    #[test]
    fn backend_resolves_to_something() {
        let b = backend();
        // Just verify the dispatch returns SOMETHING and is stable.
        assert_eq!(backend(), b);
        println!("[simd_cosine] backend: {}", b.name());
    }

    #[test]
    fn scalar_matches_dispatched_768d() {
        // Same random pair, same answer regardless of backend.
        let a = rand_vec(768, 42);
        let b = rand_vec(768, 137);
        let (d1, na1, nb1) = sums_scalar(&a, &b);
        let (d2, na2, nb2) = sums(&a, &b);
        // FMA reorders ops vs scalar adds — small accumulator drift is OK.
        assert!(approx_eq(d1, d2, 1e-3),   "dot:  scalar={} dispatched={}", d1, d2);
        assert!(approx_eq(na1, na2, 1e-3), "|a|²: scalar={} dispatched={}", na1, na2);
        assert!(approx_eq(nb1, nb2, 1e-3), "|b|²: scalar={} dispatched={}", nb1, nb2);
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[test]
    fn avx2_matches_scalar_when_available() {
        if !std::arch::is_x86_feature_detected!("avx2")
            || !std::arch::is_x86_feature_detected!("fma") {
            eprintln!("AVX2+FMA not available on this host — skipping");
            return;
        }
        let a = rand_vec(768, 7);
        let b = rand_vec(768, 11);
        let (d1, _, _) = sums_scalar(&a, &b);
        let (d2, _, _) = unsafe { sums_avx2(&a, &b) };
        assert!(approx_eq(d1, d2, 1e-3));
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[test]
    fn avx512_matches_scalar_when_available() {
        if !std::arch::is_x86_feature_detected!("avx512f") {
            eprintln!("AVX-512F not available on this host — skipping");
            return;
        }
        let a = rand_vec(768, 17);
        let b = rand_vec(768, 23);
        let (d1, _, _) = sums_scalar(&a, &b);
        let (d2, _, _) = unsafe { sums_avx512(&a, &b) };
        assert!(approx_eq(d1, d2, 1e-3));
    }
}
