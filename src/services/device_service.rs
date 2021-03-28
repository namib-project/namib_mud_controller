pub use futures::TryStreamExt;
use sqlx::Done;

use namib_shared::models::DhcpLeaseInformation;

use crate::{
    db::DbConnection,
    error::{Error, Result},
    models::{Device, DeviceDbo},
    services::{
        config_service, config_service::ConfigKeys, firewall_configuration_service, mud_service,
        mud_service::get_or_fetch_mud, neo4jthings_service, room_service,
    },
};

pub async fn upsert_device_from_dhcp_lease(lease_info: DhcpLeaseInformation, pool: &DbConnection) -> Result<()> {
    let mut dhcp_device_data = Device::from(lease_info);
    let update = if let Ok(device) = find_by_ip(dhcp_device_data.ip_addr, false, pool).await {
        dhcp_device_data.id = device.id;
        dhcp_device_data.collect_info = device.collect_info;
        dhcp_device_data.room = device.room;
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
    let devices = sqlx::query_as!(DeviceDbo, "select * from devices").fetch(pool);

    let devices_data = devices
        .err_into::<Error>()
        .and_then(|device| async {
            let room = match device.room_id {
                Some(id) => Some(room_service::find_by_id(id, pool).await?),
                None => None,
            };
            let mut device_data = Device::from_dbo(device, room);
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
    let device: DeviceDbo = sqlx::query_as!(DeviceDbo, "select * from devices where id = ?", id)
        .fetch_one(pool)
        .await?;

    let room = match device.room_id {
        Some(id) => Some(room_service::find_by_id(id, pool).await?),
        None => None,
    };
    Ok(Device::from_dbo(device, room))
}

pub async fn find_by_ip(ip_addr: std::net::IpAddr, fetch_mud: bool, pool: &DbConnection) -> Result<Device> {
    let ip_addr = ip_addr.to_string();
    let device: DeviceDbo = sqlx::query_as!(DeviceDbo, "select * from devices where ip_addr = ?", ip_addr)
        .fetch_one(pool)
        .await?;

    let room = match device.room_id {
        Some(id) => Some(room_service::find_by_id(id, pool).await?),
        None => None,
    };
    let mut device = Device::from_dbo(device, room);

    if fetch_mud && device.mud_url.is_some() {
        device.mud_data = Some(mud_service::get_or_fetch_mud(device.mud_url.as_ref().unwrap(), pool).await?);
    }

    Ok(device)
}

pub async fn insert_device(device_data: &Device, pool: &DbConnection) -> Result<u64> {
    let ip_addr = device_data.ip_addr.to_string();
    let mac_addr = device_data.mac_addr.map(|m| m.to_string());

    let mut room_id: Option<i64> = None;
    if let Some(r) = &device_data.room {
        room_service::update(&r.clone(), pool).await?;
        room_id = Some(r.room_id);
    }

    let ins_count = sqlx::query!(
        "insert into devices (ip_addr, mac_addr, hostname, vendor_class, mud_url, collect_info, last_interaction, room_id, clipart) values (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        ip_addr,
        mac_addr,
        device_data.hostname,
        device_data.vendor_class,
        device_data.mud_url,
        device_data.collect_info,
        device_data.last_interaction,
        room_id,
        device_data.clipart,
    )
    .execute(pool)
    .await?;

    if device_data.collect_info {
        // add the device in the background as it may take some time
        tokio::spawn(neo4jthings_service::add_device(device_data.clone()));
    }

    Ok(ins_count.rows_affected())
}

pub async fn update_device(device_data: &Device, pool: &DbConnection) -> Result<bool> {
    let ip_addr = device_data.ip_addr.to_string();
    let mac_addr = device_data.mac_addr.map(|m| m.to_string());

    let mut room_id: Option<i64> = None;
    if let Some(r) = &device_data.room {
        room_service::update(&r.clone(), pool).await?;
        room_id = Some(r.room_id);
    }

    let upd_count = sqlx::query!(
        "update devices set ip_addr = ?, mac_addr = ?, hostname = ?, vendor_class = ?, mud_url = ?, collect_info = ?, last_interaction = ?, room_id = ?, clipart = ? where id = ?",
        ip_addr,
        mac_addr,
        device_data.hostname,
        device_data.vendor_class,
        device_data.mud_url,
        device_data.collect_info,
        device_data.last_interaction,
        room_id,
        device_data.clipart,
        device_data.id,
    )
    .execute(pool)
    .await?;

    Ok(upd_count.rows_affected() == 1)
}

pub async fn delete_device(id: i64, pool: &DbConnection) -> Result<bool> {
    let del_count = sqlx::query!("delete from devices where id = ?", id)
        .execute(pool)
        .await?;

    Ok(del_count.rows_affected() == 1)
}
