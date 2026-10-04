use utoipa::OpenApi;

use crate::app::{__path_healthz, __path_readyz};
use crate::routes::auth::{
    __path_forgot_password, __path_login, __path_logout, __path_logout_all, __path_refresh,
    __path_register, __path_reset_password,
};
use crate::routes::links::{
    __path_create_link, __path_list_links, __path_revoke_link, __path_update_link,
};
use crate::routes::me::{__path_me, __path_update_me};
use crate::routes::recordings::{
    __path_create_recording, __path_download_recording, __path_list_recordings,
    __path_rename_recording, __path_retry_recording, __path_trash_recording,
};
use crate::routes::takes::{
    __path_ack_chunk, __path_finalize_take, __path_presign_chunks, __path_take_status,
};
use crate::routes::verify_email::__path_verify_email;
use crate::routes::watch::{__path_download, __path_playback, __path_watch};

/// The API contract. `api openapi` prints it; `just openapi` writes it to
/// `docs/api/openapi.json` and generates `web/src/app/api/schema.ts` from it.
#[derive(OpenApi)]
#[openapi(
    info(title = "Sintade API", version = "0.1.0"),
    paths(
        healthz,
        readyz,
        register,
        login,
        refresh,
        logout,
        logout_all,
        verify_email,
        forgot_password,
        reset_password,
        me,
        update_me,
        create_recording,
        list_recordings,
        rename_recording,
        trash_recording,
        retry_recording,
        create_link,
        list_links,
        update_link,
        revoke_link,
        watch,
        playback,
        download,
        download_recording,
        presign_chunks,
        ack_chunk,
        take_status,
        finalize_take,
    )
)]
pub struct ApiDoc;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes;

    /// Every mounted route must be in the contract, so the generated TS DTOs never miss one.
    #[test]
    fn every_route_in_the_table_is_documented() {
        let doc = ApiDoc::openapi();
        for route in routes::table() {
            let item = doc
                .paths
                .paths
                .get(route.path)
                .unwrap_or_else(|| panic!("{} missing from ApiDoc", route.path));
            let documented = match route.method.as_str() {
                "GET" => item.get.is_some(),
                "POST" => item.post.is_some(),
                "PATCH" => item.patch.is_some(),
                "PUT" => item.put.is_some(),
                "DELETE" => item.delete.is_some(),
                other => panic!("unexpected method {other}"),
            };
            assert!(
                documented,
                "{} {} missing from ApiDoc",
                route.method, route.path
            );
        }
    }
}
