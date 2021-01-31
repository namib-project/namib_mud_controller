#![allow(clippy::needless_pass_by_value)]

use crate::{db::DbConnection, error::Result, models::MudData, services::mud_service};
use paperclip::actix::{api_v2_operation, web, web::Json};

pub fn init(cfg: &mut web::ServiceConfig) {
    cfg.route("/{url}", web::get().to(get_mud));
}

// TODO: remove this
#[api_v2_operation]
pub async fn get_mud(pool: web::Data<DbConnection>, url: web::Path<String>) -> Result<Json<MudData>> {
    let res = mud_service::get_mud_from_url(url.into_inner(), pool.get_ref()).await?;
    info!("{:?}", res);
    Ok(Json(res))
}
