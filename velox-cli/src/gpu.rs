use serde::Deserialize;
use std::process::Command;
use std::time::{Duration, Instant};
use wmi::{COMLibrary, WMIConnection};

pub struct GpuSample {
    pub utilization: f64,
    pub mem_used: f64,
    pub temperature: f64,
}

#[derive(Deserialize, Debug)]
#[serde(rename = "Win32_VideoController")]
pub struct Gpu {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "AdapterRAM")]
    pub adapter_ram: Option<u32>,
}

pub fn get_gpu_info(wmi: &WMIConnection) -> Vec<Gpu> {
    wmi.query().unwrap_or_default()
}

fn query_nvidia_smi() -> Option<Vec<String>> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    Some(text.lines().map(|l| l.to_string()).collect())
}

pub fn run_status() {
    println!("=== APEX Velox — gpu status ===\n");

    if let Some(lines) = query_nvidia_smi() {
        for line in lines {
            let parts: Vec<&str> = line.split(',').map(|p| p.trim()).collect();
            if parts.len() != 5 {
                continue;
            }
            println!("GPU:         {}", parts[0]);
            println!("Utilization: {}%", parts[1]);
            println!("VRAM:        {} / {} MiB", parts[2], parts[3]);
            println!("Temperature: {}°C", parts[4]);
        }
        return;
    }

    println!("nvidia-smi not available — falling back to WMI (name only, no live usage)\n");
    let Ok(com) = COMLibrary::new() else {
        println!("GPU 정보 조회 실패 (WMI 접근 불가).");
        return;
    };
    let Ok(wmi) = WMIConnection::new(com) else {
        println!("GPU 정보 조회 실패 (WMI 연결 불가).");
        return;
    };
    let gpus = get_gpu_info(&wmi);
    if gpus.is_empty() {
        println!("GPU 정보 없음.");
    }
    for gpu in gpus {
        println!("GPU: {}", gpu.name);
        if let Some(ram) = gpu.adapter_ram {
            println!("VRAM (total): {} MiB", ram / 1024 / 1024);
        }
    }
}

/// Poll nvidia-smi roughly twice per second for `seconds`, returning each sample.
pub fn sample_nvidia_smi(seconds: u64) -> Option<Vec<GpuSample>> {
    // Probe once first; if nvidia-smi is missing, bail immediately.
    query_nvidia_smi()?;

    let mut samples = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        if let Some(lines) = query_nvidia_smi()
            && let Some(line) = lines.first()
        {
            let parts: Vec<&str> = line.split(',').map(|p| p.trim()).collect();
            if parts.len() == 5 {
                samples.push(GpuSample {
                    utilization: parts[1].parse().unwrap_or(0.0),
                    mem_used: parts[2].parse().unwrap_or(0.0),
                    temperature: parts[4].parse().unwrap_or(0.0),
                });
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Some(samples)
}
