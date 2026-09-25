use utoipa::OpenApi;

use crate::app::{__path_healthz, __path_readyz};
use crate::routes::auth::{
    __path_forgot_password, __path_login, __path_logout, __path_logout_all, __path_refresh,
    __path_register, __path_reset_password,
};
use crate::routes::me::{__path_me, __path_update_me};
use crate::routes::verify_email::__path_verify_email;

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
