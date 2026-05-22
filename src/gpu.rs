use wmi::WMIConnection;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
#[serde(rename = "Win32_VideoController")]
pub struct GPU {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "AdapterRAM")]
    pub adapter_ram: Option<u32>,
}

pub fn get_gpu_info(wmi: &WMIConnection) -> Vec<GPU> {
    wmi.query().unwrap_or_default()
}