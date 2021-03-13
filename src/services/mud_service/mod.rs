use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use isahc::AsyncReadResponseExt;
use lazy_static::lazy_static;
use regex::Regex;

use crate::{
    db::DbConnection,
    error::Result,
    models::{Acl, MudData, MudDbo, MudDboRefresh},
};
use sqlx::Done;

mod json_models;
pub mod mud_profile_service;
mod parser;

/// Writes the `MudDbo` to the database.
/// Upserts data by `MudDbo::url`
pub async fn upsert_mud(mud_profile: &MudDbo, pool: &DbConnection) -> Result<()> {
    let _ins_count = sqlx::query!(
        "INSERT INTO mud_data (url, data, created_at, expiration, acl_override) VALUES (?, ?, ?, ?, ?) ON CONFLICT(url) DO UPDATE SET data = excluded.data, created_at = excluded.created_at, expiration = excluded.expiration, acl_override = excluded.acl_override",
        mud_profile.url,
        mud_profile.data,
        mud_profile.created_at,
        mud_profile.expiration,
        mud_profile.acl_override,
    )
        .execute(pool)
        .await?;

    // Ok(ins_count.rows_affected())
    Ok(())
}

/// Creates MUD Profile using `MudDbo` Data
pub async fn create_mud(mud_profile: &MudDbo, pool: &DbConnection) -> Result<()> {
    let _ins_count = sqlx::query!(
        "INSERT INTO mud_data (url, data, created_at, expiration, acl_override) VALUES (?, ?, ?, ?, ?)",
        mud_profile.url,
        mud_profile.data,
        mud_profile.created_at,
        mud_profile.expiration,
        mud_profile.acl_override,
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// Returns the MUD-Profile if it exists
pub async fn get_mud(url: &str, pool: &DbConnection) -> Option<MudDbo> {
    sqlx::query_as!(MudDbo, "SELECT * FROM mud_data WHERE url = ?", url)
        .fetch_optional(pool)
        .await
        .ok()?
}

/// Returns all existing MUD-Profiles
pub async fn get_all_muds(pool: &DbConnection) -> Result<Vec<MudDbo>> {
    let data = sqlx::query_as!(MudDbo, "SELECT * FROM mud_data")
        .fetch_all(pool)
        .await?;

    Ok(data)
}

/// Deletes a MUD-Profile using the MUD-URL/-Name
pub async fn delete_mud(url: &str, pool: &DbConnection) -> Result<u64> {
    let del_count = sqlx::query!("DELETE FROM mud_data WHERE url = ?", url)
        .execute(pool)
        .await?;

    Ok(del_count.rows_affected())
}

/// Checks if a Device is using the MUD-Profile
pub async fn is_mud_used(url: &str, pool: &DbConnection) -> Result<bool> {
    let device_using_mud = sqlx::query!("SELECT * FROM devices WHERE mud_url = ?", url)
        .fetch_optional(pool)
        .await?;

    Ok(device_using_mud.is_some())
}

/// Returns a MUD-Profile Data using the MUD-URL
/// Creates the MUD-Profile if it doesn't exist
/// If it exists, uses existing data from DB
/// This function is mainly used in the `RPCServer`, where it's used to save Device's MUD-URLs which are being sent via DHCP
/// Local MUD-Profiles can be loaded *BUT NOT CREATED* through this function, since they have an expiration far far in the future
pub async fn get_mud_from_url(url: String, pool: &DbConnection) -> Result<MudData> {
    // lookup datenbank ob schon existiert und nicht abgelaufen
    let existing_mud = get_mud(&url, pool).await;

    if let Some(mud) = existing_mud {
        if mud.expiration > Utc::now().naive_utc() {
            if let Ok(mud) = serde_json::from_str::<MudData>(mud.data.as_str()) {
                return Ok(mud);
            }
        }
    }

    // wenn nicht: fetch
    let mud_json = fetch_mud(url.as_str()).await?;

    // ruf parse_mud auf
    let data = parser::parse_mud(url.clone(), mud_json.as_str())?;

    // speichern in db
    let mud = MudDbo {
        url: url.clone(),
        data: serde_json::to_string(&data)?,
        acl_override: None,
        created_at: Utc::now().naive_utc(),
        expiration: data.expiration.naive_utc(),
    };

    debug!("new/updating mud profile: {:?}", mud);

    upsert_mud(&mud, pool).await?;

    // return muddata
    Ok(data)
}

/// Basic HTTP(S)-GET wrapper to fetch MUD-URL Data
async fn fetch_mud(url: &str) -> Result<String> {
    Ok(isahc::get_async(url).await?.text().await?)
}

/// Checks if the given string is an URL. Used to check if the MUD-Profile being created is local or needs to be fetched.
pub fn is_url(url: &str) -> bool {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"https?://(-\.)?([^\s/?\.#-]+\.?)+(/[^\s]*)?$").unwrap();
    }
    RE.is_match(url)
}

/// Generates an empty MUD-Profile for custom local usage
pub fn generate_empty_custom_mud_profile(url: &str, acl_override: Option<Vec<Acl>>) -> MudData {
    MudData {
        url: url.to_string(),
        masa_url: None,
        last_update: Utc::now().naive_local().to_string(),
        systeminfo: None,
        mfg_name: None,
        model_name: None,
        documentation: None,
        expiration: get_custom_mud_expiration(),
        acllist: vec![],
        acl_override,
    }
}

/// Generates an expiration date, which is far in the future. Mainly used for custom local MUD-Profiles
pub fn get_custom_mud_expiration() -> DateTime<Utc> {
    Utc.from_utc_datetime(&NaiveDate::from_ymd(2060, 1, 31).and_hms(0, 0, 0))
}

/// This function return MudDboRefresh they only containing url and expiration
/// to reduce payload.
async fn get_all_mud_expiration(pool: &DbConnection) -> Result<Vec<MudDboRefresh>> {
    Ok(sqlx::query_as!(MudDboRefresh, "select url, expiration from mud_data")
        .fetch_all(pool)
        .await?)
}
