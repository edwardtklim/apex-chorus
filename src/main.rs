mod chorus;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "velox")]
#[command(about = "APEX Velox — system monitor and AI orchestrator")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Info,
    Chorus {
        #[command(subcommand)]
        action: ChorusCommands,
    },
}

#[derive(Subcommand)]
enum ChorusCommands {
    Ask {
        prompt: String,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Info => run_info(),
        Commands::Chorus { action } => match action {
            ChorusCommands::Ask { prompt } => {
                chorus::ask(&prompt).await;
            }
        },
    }
}

fn run_info() {
    use wmi::{COMLibrary, WMIConnection};
    use serde::Deserialize;

    #[derive(Deserialize, Debug)]
    #[serde(rename = "Win32_Processor")]
    struct Processor {
        #[serde(rename = "Name")]
        name: String,
        #[serde(rename = "NumberOfCores")]
        number_of_cores: u32,
    }

    #[derive(Deserialize, Debug)]
    #[serde(rename = "Win32_VideoController")]
    struct GPU {
        #[serde(rename = "Name")]
        name: String,
    }

    #[derive(Deserialize, Debug)]
    #[serde(rename = "Win32_Battery")]
    struct Battery {
        #[serde(rename = "EstimatedChargeRemaining")]
        charge: u32,
    }

    #[derive(Deserialize, Debug)]
    #[serde(rename = "MSAcpi_ThermalZoneTemperature")]
    struct ThermalZone {
        #[serde(rename = "CurrentTemperature")]
        current_temperature: u32,
        #[serde(rename = "InstanceName")]
        instance_name: String,
    }

    println!("=== APEX Velox — velox info ===\n");

    let com = COMLibrary::new().expect("COM init failed");
    let wmi = WMIConnection::new(com.clone()).expect("WMI connection failed");

    let cpus: Vec<Processor> = wmi.query().expect("CPU query failed");
    for cpu in &cpus {
        println!("CPU:    {}", cpu.name);
        println!("Cores:  {}", cpu.number_of_cores);
    }

    println!();

    let gpus: Vec<GPU> = wmi.query().expect("GPU query failed");
    for gpu in &gpus {
        println!("GPU:    {}", gpu.name);
    }

    println!();

    let batteries: Vec<Battery> = wmi.query().expect("Battery query failed");
    if batteries.is_empty() {
        println!("Battery: No battery detected");
    } else {
        for bat in &batteries {
            println!("Battery: {}%", bat.charge);
        }
    }

    println!();
    println!("--- Temperatures ---");
    let wmi2 = WMIConnection::with_namespace_path("ROOT\\WMI", com)
        .expect("WMI ROOT\\WMI failed");

    let temps: Vec<ThermalZone> = wmi2.query().unwrap_or_default();
    if temps.is_empty() {
        println!("Temperature: Run as Administrator for sensor data");
    } else {
        for t in &temps {
            let celsius = (t.current_temperature as f32 / 10.0) - 273.15;
            println!("{}: {:.1}°C", t.instance_name, celsius);
        }
    }

    println!("\n=== velox info complete ===");
}