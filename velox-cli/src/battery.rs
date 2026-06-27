use wmi::WMIConnection;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
#[serde(rename = "Win32_Battery")]
pub struct Battery {
    #[serde(rename = "EstimatedChargeRemaining")]
    pub charge: u32,
}

pub fn get_battery_info(wmi: &WMIConnection) -> Vec<Battery> {
    wmi.query().unwrap_or_default()
}