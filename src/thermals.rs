use wmi::WMIConnection;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
#[serde(rename = "MSAcpi_ThermalZoneTemperature")]
pub struct ThermalZone {
    #[serde(rename = "CurrentTemperature")]
    pub current_temperature: u32,
    #[serde(rename = "InstanceName")]
    pub instance_name: String,
}

pub fn get_thermals(wmi: &WMIConnection) -> Vec<ThermalZone> {
    wmi.query().unwrap_or_default()
}

pub fn to_celsius(raw: u32) -> f32 {
    (raw as f32 / 10.0) - 273.15
}