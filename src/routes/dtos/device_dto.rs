use crate::models::{device_model::Device, mud_models::MUDData};
use chrono::NaiveDateTime;
use paperclip::actix::Apiv2Schema;

#[derive(Debug, Serialize, Deserialize, Apiv2Schema)]
pub struct DeviceDto {
    pub id: i64,
    pub ip_addr: String,
    pub mac_addr: Option<String>,
    pub hostname: String,
    pub vendor_class: String,
    pub mud_url: Option<String>,
    pub last_interaction: NaiveDateTime,
    pub mud_data: Option<MUDData>,
}

impl From<Device> for DeviceDto {
    fn from(d: Device) -> Self {
        DeviceDto {
            id: d.id,
            ip_addr: d.ip_addr.to_string(),
            mac_addr: d.mac_addr.map(|m| m.to_string()),
            hostname: d.hostname,
            vendor_class: d.vendor_class,
            mud_url: d.mud_url,
            last_interaction: d.last_interaction,
            mud_data: d.mud_data,
        }
    }
}
