use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const PRIME_LIMIT: u64 = 8_000_000;
const MATMUL_N: usize = 320;

fn sieve_count(limit: u64) -> u64 {
    let limit = limit as usize;
    let mut is_prime = vec![true; limit + 1];
    is_prime[0] = false;
    if limit >= 1 {
        is_prime[1] = false;
    }
    let mut i = 2usize;
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
    is_prime.iter().filter(|&&p| p).count() as u64
}

fn matmul(n: usize) -> f64 {
    let a: Vec<f64> = (0..n * n).map(|i| (i % 97) as f64 * 0.5).collect();
    let b: Vec<f64> = (0..n * n).map(|i| (i % 89) as f64 * 0.3).collect();
    let mut c = vec![0.0f64; n * n];

    for i in 0..n {
        for k in 0..n {
            let a_ik = a[i * n + k];
            for j in 0..n {
                c[i * n + j] += a_ik * b[k * n + j];
            }
        }
    }
    c.iter().sum()
}

fn available_cores() -> usize {
    thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}

/// 타임라인 기록용 빠른 점수 (single GFLOPS, multi GFLOPS). 고정 작업량 = 비교 가능.
pub fn quick_score() -> (f64, f64) {
    let cores = available_cores();
    let n = MATMUL_N;
    let flops = 2.0 * (n as f64).powi(3);
    let runs: usize = 20;

    let t = Instant::now();
    for _ in 0..runs {
        let _ = matmul(n);
    }
    let single = flops * runs as f64 / t.elapsed().as_secs_f64() / 1e9;

    let t = Instant::now();
    thread::scope(|s| {
        for _ in 0..cores {
            s.spawn(|| {
                for _ in 0..runs {
                    let _ = matmul(n);
                }
            });
        }
    });
    let multi = flops * (runs * cores) as f64 / t.elapsed().as_secs_f64() / 1e9;

    (single, multi)
}

// ---------------- 지속 성능(stability) 벤치 — CPU 쓰로틀링 측정 ----------------

fn flops_per_matmul(n: usize) -> f64 {
    2.0 * (n as f64).powi(3)
}

/// 싱글스레드로 1초 단위 GFLOPS를 seconds 동안 측정 (성능 곡선).
fn stability_single(seconds: u64, n: usize) -> Vec<f64> {
    let unit = flops_per_matmul(n);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut buckets = Vec::new();
    while Instant::now() < deadline {
        let s = Instant::now();
        let mut flops = 0.0;
        while s.elapsed() < Duration::from_secs(1) {
            let _ = matmul(n);
            flops += unit;
        }
        buckets.push(flops / s.elapsed().as_secs_f64() / 1e9);
    }
    buckets
}

/// 멀티스레드: 모든 코어가 계속 matmul, 1초 단위로 누적 처리량을 샘플링.
fn stability_multi(seconds: u64, n: usize, cores: usize) -> Vec<f64> {
    let unit = flops_per_matmul(n);
    let count = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));

    thread::scope(|s| {
        for _ in 0..cores {
            let count = count.clone();
            let stop = stop.clone();
            s.spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let _ = matmul(n);
                    count.fetch_add(1, Ordering::Relaxed);
                }
            });
        }

        let mut buckets = Vec::new();
        let mut prev = 0u64;
        for _ in 0..seconds {
            let t = Instant::now();
            thread::sleep(Duration::from_secs(1));
            let now = count.load(Ordering::Relaxed);
            let done = now - prev;
            prev = now;
            buckets.push(done as f64 * unit / t.elapsed().as_secs_f64() / 1e9);
        }
        stop.store(true, Ordering::Relaxed);
        buckets
    })
}

fn report(label: &str, buckets: &[f64]) {
    if buckets.is_empty() {
        println!("[{}] 데이터 없음", label);
        return;
    }
    let peak = buckets.iter().cloned().fold(f64::MIN, f64::max);
    let min = buckets.iter().cloned().fold(f64::MAX, f64::min);
    let avg = buckets.iter().sum::<f64>() / buckets.len() as f64;
    let first = buckets[0];
    let last = *buckets.last().unwrap();
    let ret_avg = avg / peak * 100.0;
    let ret_min = min / peak * 100.0;
    let verdict = if ret_min >= 90.0 {
        "안정적 ✓ (쓰로틀링 거의 없음)"
    } else if ret_min >= 80.0 {
        "약간 하락 (경미한 쓰로틀링)"
    } else {
        "성능 하락 큼 ⚠ (쓰로틀링 의심)"
    };
    println!("[{}]", label);
    println!("  초당 GFLOPS: peak {:.1} · avg {:.1} · min {:.1}", peak, avg, min);
    println!("  처음→끝: {:.1} → {:.1} GFLOPS", first, last);
    println!("  유지율: 평균 {:.0}% · 최저 {:.0}%", ret_avg, ret_min);
    println!("  판정: {}", verdict);
}

pub fn run_stability(seconds: u64) {
    let cores = available_cores();
    println!("=== APEX Velox — bench stability (CPU 지속 성능) ===");
    println!("각 단계 {}초 · 논리코어 {}\n", seconds, cores);

    println!("싱글스레드 {}초 지속 부하 측정...", seconds);
    let single = stability_single(seconds, MATMUL_N);
    report("싱글스레드", &single);

    println!("\n멀티스레드({}코어) {}초 지속 부하 측정...", cores, seconds);
    let multi = stability_multi(seconds, MATMUL_N, cores);
    report("멀티스레드", &multi);

    println!("\n* 유지율 = 구간 성능 / 최고 성능. 100%에 가까울수록 발열·쓰로틀링 없이 일정.");
}

fn run_workload_single<F: Fn() -> T, T>(f: F) -> Duration {
    let start = Instant::now();
    f();
    start.elapsed()
}

fn run_workload_multi<F: Fn() -> T + Sync, T: Send>(f: F, threads: usize) -> Duration {
    let start = Instant::now();
    thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| f());
        }
    });
    start.elapsed()
}

fn score_from(duration: Duration, work_units: f64) -> f64 {
    work_units / duration.as_secs_f64()
}

pub fn run_cpu() {
    let cores = available_cores();
    println!("=== APEX Velox — benchmark cpu ===");
    println!("Logical cores: {}\n", cores);

    println!("-- Single-thread --");
    let t = run_workload_single(|| sieve_count(PRIME_LIMIT));
    let single_prime_score = score_from(t, PRIME_LIMIT as f64) / 1_000_000.0;
    println!("Prime sieve ({} numbers): {:.2?}  ->  {:.2} M ops/sec", PRIME_LIMIT, t, single_prime_score);

    let t = run_workload_single(|| matmul(MATMUL_N));
    let flops = 2.0 * (MATMUL_N as f64).powi(3);
    let single_matmul_gflops = flops / t.as_secs_f64() / 1e9;
    println!("Matrix multiply ({0}x{0}): {1:.2?}  ->  {2:.2} GFLOPS", MATMUL_N, t, single_matmul_gflops);

    println!("\n-- Multi-thread ({} threads) --", cores);
    let t = run_workload_multi(|| sieve_count(PRIME_LIMIT), cores);
    let multi_prime_score = score_from(t, PRIME_LIMIT as f64 * cores as f64) / 1_000_000.0;
    println!("Prime sieve x{}: {:.2?}  ->  {:.2} M ops/sec  ({:.1}x scaling)", cores, t, multi_prime_score, multi_prime_score / single_prime_score);

    let t = run_workload_multi(|| matmul(MATMUL_N), cores);
    let multi_matmul_gflops = flops * cores as f64 / t.as_secs_f64() / 1e9;
    println!("Matrix multiply x{}: {:.2?}  ->  {:.2} GFLOPS  ({:.1}x scaling)", cores, t, multi_matmul_gflops, multi_matmul_gflops / single_matmul_gflops);

    println!("\n-- Composite score (arbitrary, single-thread = 1000 baseline) --");
    let base = single_prime_score + single_matmul_gflops;
    println!("Single-core: 1000");
    println!("Multi-core:  {:.0}", 1000.0 * (multi_prime_score + multi_matmul_gflops) / base);
}

pub fn run_gpu_monitor(seconds: u64) {
    println!("=== APEX Velox — benchmark gpu (monitor) ===");
    println!("Note: this samples real-time GPU stats via nvidia-smi.");
    println!("It does NOT generate GPU load itself — run a game, render job,");
    println!("or AI workload at the same time to see real numbers.\n");

    let samples = crate::gpu::sample_nvidia_smi(seconds);
    match samples {
        Some(stats) if !stats.is_empty() => {
            let utils: Vec<f64> = stats.iter().map(|s| s.utilization).collect();
            let temps: Vec<f64> = stats.iter().map(|s| s.temperature).collect();
            let mems: Vec<f64> = stats.iter().map(|s| s.mem_used).collect();

            let avg = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
            let max = |v: &[f64]| v.iter().cloned().fold(f64::MIN, f64::max);

            println!("Samples: {}", stats.len());
            println!("Utilization: avg {:.1}%  peak {:.1}%", avg(&utils), max(&utils));
            println!("Temperature: avg {:.1}°C  peak {:.1}°C", avg(&temps), max(&temps));
            println!("VRAM used:   avg {:.0} MiB  peak {:.0} MiB", avg(&mems), max(&mems));
        }
        _ => println!("nvidia-smi not available on this system."),
    }
}

pub fn run_everyday() {
    println!("=== APEX Velox — benchmark everyday ===\n");
    run_compress();
    println!();
    run_image();
}

fn run_compress() {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    let size = 64 * 1024 * 1024;
    let mut data = Vec::with_capacity(size);
    let pattern = b"APEX Velox benchmark payload - everyday compression test. ";
    while data.len() < size {
        data.extend_from_slice(pattern);
    }
    data.truncate(size);

    let start = Instant::now();
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&data).unwrap();
    let compressed = encoder.finish().unwrap();
    let elapsed = start.elapsed();

    let throughput = (size as f64 / 1024.0 / 1024.0) / elapsed.as_secs_f64();
    println!("-- File compression (64 MB, gzip) --");
    println!("Time: {:.2?}  ->  {:.1} MB/s  (compressed to {:.1} MB)", elapsed, throughput, compressed.len() as f64 / 1024.0 / 1024.0);
}

fn run_image() {
    use image::{ImageBuffer, Rgb};

    let (w, h) = (4000u32, 3000u32);
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(w, h, |x, y| {
        Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
    });

    let iterations = 10;
    let start = Instant::now();
    for _ in 0..iterations {
        let resized = image::imageops::resize(&img, w / 4, h / 4, image::imageops::FilterType::Lanczos3);
        let mut buf = Vec::new();
        resized
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Jpeg)
            .unwrap();
    }
    let elapsed = start.elapsed();

    println!("-- Image resize + JPEG encode (4000x3000 -> 1000x750, x{}) --", iterations);
    println!("Time: {:.2?}  ->  {:.2} images/sec", elapsed, iterations as f64 / elapsed.as_secs_f64());
}
