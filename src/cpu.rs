use wmi::WMIConnection;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
#[serde(rename = "Win32_Processor")]
pub struct Processor {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "CurrentClockSpeed")]
    pub current_clock_speed: u32,
    #[serde(rename = "NumberOfCores")]
    pub number_of_cores: u32,
}

pub fn get_cpu_info(wmi: &WMIConnection) -> Vec<Processor> {
    wmi.query().unwrap_or_default()
}