use crate::{
    db::DbConnection,
    error::{Error, Result},
    models::{Device, DeviceDbo},
    services::{
        config_service, config_service::ConfigKeys, firewall_configuration_service, mud_service,
        mud_service::get_or_fetch_mud, neo4jthings_service,
    },
};
pub use futures::TryStreamExt;

use namib_shared::models::DhcpLeaseInformation;
use sqlx::Done;

pub async fn upsert_device_from_dhcp_lease(lease_info: DhcpLeaseInformation, pool: &DbConnection) -> Result<()> {
    let mut dhcp_device_data = Device::from(lease_info);
    let update = if let Ok(device) = find_by_ip(dhcp_device_data.ip_addr, false, pool).await {
        dhcp_device_data.id = device.id;
        dhcp_device_data.collect_info = device.collect_info;
        true
    } else {
        dhcp_device_data.collect_info = dhcp_device_data.mud_url.is_none()
            && config_service::get_config_value(ConfigKeys::CollectDeviceData.as_ref(), pool)
                .await
                .unwrap_or(false);
        false
    };

    debug!("dhcp request device mud file: {:?}", dhcp_device_data.mud_url);

    match &dhcp_device_data.mud_url {
        Some(url) => mud_service::get_or_fetch_mud(&url, pool).await.ok(),
        None => None,
    };
    if update {
        update_device(&dhcp_device_data, pool).await.unwrap();
    } else {
        insert_device(&dhcp_device_data, pool).await.unwrap();
    }

    firewall_configuration_service::update_config_version(pool).await?;

    Ok(())
}

pub async fn get_all_devices(pool: &DbConnection) -> Result<Vec<Device>> {
    let devices = sqlx::query_as!(DeviceDbo, "SELECT * FROM devices").fetch(pool);

    let devices_data = devices
        .err_into::<Error>()
        .and_then(|device| async {
            let mut device_data = Device::from(device);
            device_data.mud_data = match device_data.mud_url.clone() {
                Some(url) => {
                    let data = get_or_fetch_mud(&url, pool).await;
                    debug!("Get all devices: mud url {:?}: {:?}", url, data);
                    data.ok()
                },
                None => None,
            };

            Ok(device_data)
        })
        .try_collect::<Vec<Device>>()
        .await?;

    Ok(devices_data)
}

pub async fn find_by_id(id: i64, pool: &DbConnection) -> Result<Device> {
    let device = sqlx::query_as!(DeviceDbo, "SELECT * FROM devices WHERE id = $1", id)
        .fetch_one(pool)
        .await?;

    Ok(Device::from(device))
}

pub async fn find_by_ip(ip_addr: std::net::IpAddr, fetch_mud: bool, pool: &DbConnection) -> Result<Device> {
    let ip_addr = ip_addr.to_string();
    let device = sqlx::query_as!(DeviceDbo, "SELECT * FROM devices WHERE ip_addr = $1", ip_addr)
        .fetch_one(pool)
        .await?;

    let mut device = Device::from(device);

    if fetch_mud && device.mud_url.is_some() {
        device.mud_data = Some(mud_service::get_or_fetch_mud(device.mud_url.as_ref().unwrap(), pool).await?);
    }

    Ok(device)
}

pub async fn insert_device(device_data: &Device, pool: &DbConnection) -> Result<bool> {
    let ip_addr = device_data.ip_addr.to_string();
    let mac_addr = device_data.mac_addr.map(|m| m.to_string());
    let ins_count = sqlx::query!(
        "INSERT INTO devices (ip_addr, mac_addr, hostname, vendor_class, mud_url, collect_info, last_interaction, clipart) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        ip_addr,
        mac_addr,
        device_data.hostname,
        device_data.vendor_class,
        device_data.mud_url,
        device_data.collect_info,
        device_data.last_interaction,
        device_data.clipart,
    )
    .execute(pool)
    .await?;

    if device_data.collect_info {
        // add the device in the background as it may take some time
        tokio::spawn(neo4jthings_service::add_device(device_data.clone()));
    }

    Ok(ins_count.rows_affected() == 1)
}

pub async fn update_device(device_data: &Device, pool: &DbConnection) -> Result<bool> {
    let ip_addr = device_data.ip_addr.to_string();
    let mac_addr = device_data.mac_addr.map(|m| m.to_string());
    let upd_count = sqlx::query!(
        "UPDATE devices SET ip_addr = $2, mac_addr = $3, hostname = $4, vendor_class = $5, mud_url = $6, collect_info = $7, last_interaction = $8, clipart = $9 WHERE id = $1",
        device_data.id,
        ip_addr,
        mac_addr,
        device_data.hostname,
        device_data.vendor_class,
        device_data.mud_url,
        device_data.collect_info,
        device_data.last_interaction,
        device_data.clipart,
    )
    .execute(pool)
    .await?;

    Ok(upd_count.rows_affected() == 1)
}

pub async fn delete_device(id: i64, pool: &DbConnection) -> Result<bool> {
    let del_count = sqlx::query!("DELETE FROM devices WHERE id = $1", id)
        .execute(pool)
        .await?;

    Ok(del_count.rows_affected() == 1)
}
