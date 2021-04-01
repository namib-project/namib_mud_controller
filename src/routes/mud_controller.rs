#![allow(clippy::needless_pass_by_value)]

use crate::{
    auth::AuthToken,
    db::DbConnection,
    error,
    error::Result,
    models::{MudData, MudDbo},
    routes::dtos::{MudCreationDto, MudQueryDto, MudUpdateDto, MudUpdateQueryDto},
    services::{mud_service, mud_service::is_url, role_service::Permission},
};
use actix_web::http::StatusCode;
use chrono::Utc;
use paperclip::actix::{
    api_v2_operation, web,
    web::{HttpResponse, Json},
};

pub fn init(cfg: &mut web::ServiceConfig) {
    cfg.route("/", web::get().to(get_muds));
    cfg.route("/", web::put().to(update_mud));
    cfg.route("/", web::delete().to(delete_mud));
    cfg.route("/", web::post().to(create_mud));
}

#[api_v2_operation(summary = "Get all known MUDs or query for a single MUD-Url")]
pub async fn get_muds(
    pool: web::Data<DbConnection>,
    query: web::Query<MudQueryDto>,
    auth: AuthToken,
) -> Result<Json<Vec<MudData>>> {
    auth.require_permission(Permission::mud__read)?;

    if let Some(url) = &query.mud_url {
        let mud = mud_service::get_or_fetch_mud(&url, &pool).await.or_else(|_| {
            error::ResponseError {
                status: StatusCode::NOT_FOUND,
                message: Some("Couldn't find MUD-Profile".to_string()),
            }
            .fail()
        })?;
        Ok(Json(vec![mud]))
    } else {
        auth.require_permission(Permission::mud__list)?;
        Ok(Json(
            mud_service::get_all_muds(&pool)
                .await?
                .iter()
                .map(|mud_dbo| serde_json::from_str(&mud_dbo.data))
                .collect::<serde_json::Result<_>>()?,
        ))
    }
}

#[api_v2_operation(summary = "Update the overrides on a MUD")]
pub async fn update_mud(
    pool: web::Data<DbConnection>,
    auth: AuthToken,
    query: web::Query<MudUpdateQueryDto>,
    mud_update_dto: Json<MudUpdateDto>,
) -> Result<Json<MudData>> {
    auth.require_permission(Permission::mud__write)?;

    let mut mud_dbo = mud_service::get_mud(&query.mud_url, &pool).await.ok_or_else(|| {
        error::ResponseError {
            status: StatusCode::NOT_FOUND,
            message: Some("Couldn't find MUD-Profile".to_string()),
        }
        .build()
    })?;

    // update the acl_override in mud_data
    let mut mud_data = serde_json::from_str::<MudData>(&mud_dbo.data)?;
    mud_data.acl_override = mud_update_dto.into_inner().acl_override.unwrap_or_default();

    // use the new mud_data in the existing mud_dbo
    mud_dbo.data = serde_json::to_string(&mud_data)?;

    mud_service::upsert_mud(&mud_dbo, &pool).await?;
    Ok(Json(mud_data))
}

#[api_v2_operation(summary = "Delete a MUD")]
pub async fn delete_mud(
    pool: web::Data<DbConnection>,
    auth: AuthToken,
    query: web::Query<MudUpdateQueryDto>,
) -> Result<HttpResponse> {
    auth.require_permission(Permission::mud__delete)?;

    let url = query.into_inner().mud_url;

    if mud_service::get_mud(&url, &pool).await.is_none() {
        error::ResponseError {
            status: StatusCode::NOT_FOUND,
            message: Some("No MUD-Profile with this URL".to_string()),
        }
        .fail()?;
    }

    if mud_service::is_mud_used(&url, &pool).await? {
        error::ResponseError {
            status: StatusCode::CONFLICT,
            message: Some("MUD is being used elsewhere, can't delete it.".to_string()),
        }
        .fail()?;
    }

    mud_service::delete_mud(&url, &pool).await?;
    Ok(HttpResponse::NoContent().finish())
}

#[api_v2_operation(summary = "Create a MUD from a URL or custom name")]
pub async fn create_mud(
    pool: web::Data<DbConnection>,
    auth: AuthToken,
    mud_creation_dto: Json<MudCreationDto>,
) -> Result<Json<MudData>> {
    auth.require_permission(Permission::mud__create)?;

    let mud_creation_dto = mud_creation_dto.into_inner();

    if mud_service::get_mud(&mud_creation_dto.mud_url, &pool).await.is_some() {
        error::ResponseError {
            status: StatusCode::CONFLICT,
            message: Some("MUD-URL key already exists".to_string()),
        }
        .fail()?;
    }

    // Check if the mud_url is actually an url. It might be a custom user mud-profile
    if is_url(&mud_creation_dto.mud_url) {
        let created_mud = mud_service::get_or_fetch_mud(&mud_creation_dto.mud_url, &pool).await?;

        Ok(Json(created_mud))
    } else {
        let empty_mud = mud_service::generate_empty_custom_mud_profile(
            &mud_creation_dto.mud_url,
            mud_creation_dto.acl_override.unwrap_or_default(),
        );
        let mud_dbo = MudDbo {
            url: mud_creation_dto.mud_url,
            data: serde_json::to_string(&empty_mud)?,
            created_at: Utc::now().naive_local(),
            expiration: empty_mud.expiration.naive_local(),
        };

        mud_service::create_mud(&mud_dbo, &pool).await?;

        Ok(Json(empty_mud))
    }
}
