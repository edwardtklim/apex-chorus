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
