//! Portable CPU benchmark engine. It returns measurements; interfaces render them.

use serde::{Deserialize, Serialize};
use std::thread;
use std::time::Instant;

const PRIME_LIMIT: u64 = 8_000_000;
const MATMUL_N: usize = 320;
const REF_SINGLE: f64 = 380.0;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CpuBenchmarkReport {
    pub version: String,
    pub logical_cores: usize,
    pub single_prime_mops: f64,
    pub single_matmul_gflops: f64,
    pub multi_prime_mops: f64,
    pub multi_matmul_gflops: f64,
    pub single_score: f64,
    pub multi_score: f64,
}

fn sieve_count(limit: u64) -> u64 {
    let limit = limit as usize;
    let mut is_prime = vec![true; limit + 1];
    is_prime[0] = false;
    if limit >= 1 {
        is_prime[1] = false;
    }
    let mut i = 2;
    while i * i <= limit {
        if is_prime[i] {
            let mut j = i * i;
            while j <= limit {
                is_prime[j] = false;
                j += i;
            }
        }
        i += 1;
    }
    is_prime.iter().filter(|&&value| value).count() as u64
}

fn matmul(n: usize) -> f64 {
    let a: Vec<f64> = (0..n * n).map(|i| (i % 97) as f64 * 0.5).collect();
    let b: Vec<f64> = (0..n * n).map(|i| (i % 89) as f64 * 0.3).collect();
    let mut c = vec![0.0_f64; n * n];
    for i in 0..n {
        for k in 0..n {
            let aik = a[i * n + k];
            for j in 0..n {
                c[i * n + j] += aik * b[k * n + j];
            }
        }
    }
    c.iter().sum()
}

fn timed<T>(f: impl FnOnce() -> T) -> f64 {
    let start = Instant::now();
    std::hint::black_box(f());
    start.elapsed().as_secs_f64().max(f64::EPSILON)
}

impl CpuBenchmarkReport {
    pub fn run() -> Self {
        let cores = thread::available_parallelism().map_or(1, usize::from);
        let single_prime_mops = PRIME_LIMIT as f64 / timed(|| sieve_count(PRIME_LIMIT)) / 1e6;
        let flops = 2.0 * (MATMUL_N as f64).powi(3);
        let single_matmul_gflops = flops / timed(|| matmul(MATMUL_N)) / 1e9;

        let start = Instant::now();
        thread::scope(|scope| {
            for _ in 0..cores {
                scope.spawn(|| std::hint::black_box(sieve_count(PRIME_LIMIT)));
            }
        });
        let multi_prime_mops = PRIME_LIMIT as f64 * cores as f64
            / start.elapsed().as_secs_f64().max(f64::EPSILON)
            / 1e6;

        let start = Instant::now();
        thread::scope(|scope| {
            for _ in 0..cores {
                scope.spawn(|| std::hint::black_box(matmul(MATMUL_N)));
            }
        });
        let multi_matmul_gflops =
            flops * cores as f64 / start.elapsed().as_secs_f64().max(f64::EPSILON) / 1e9;
        Self {
            version: env!("CARGO_PKG_VERSION").into(),
            logical_cores: cores,
            single_prime_mops,
            single_matmul_gflops,
            multi_prime_mops,
            multi_matmul_gflops,
            single_score: (single_prime_mops + single_matmul_gflops) / REF_SINGLE * 10_000.0,
            multi_score: (multi_prime_mops + multi_matmul_gflops) / REF_SINGLE * 10_000.0,
        }
    }
}
