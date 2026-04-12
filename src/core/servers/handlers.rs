use crate::{
    core::{
        models::Server,
        servers::{dto::ServerFilterDto, service::ServerService},
    },
    utils::{
        response::ApiResult,
        validation::{AuthContext, ValidatedQuery},
    },
};

pub async fn list(
    AuthContext { station, .. }: AuthContext,
    ValidatedQuery(filter): ValidatedQuery<ServerFilterDto>,
) -> ApiResult<Vec<Server>> {
    tracing::debug!("Listing servers with filter: {:?}", filter);
    ServerService::list(station, filter).await
}
