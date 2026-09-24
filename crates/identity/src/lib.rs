#![deny(clippy::unwrap_used)]

mod app;
mod domain;
mod infra;

pub use app::{
    ACCESS_TOKEN_TTL, AccessClaims, AccessTokenError, ForgotPasswordRequest, IdentityService,
    LoginError, LoginRequest, LoginSession, MeUser, MeView, MeWorkspace, REFRESH_TOKEN_TTL,
    RefreshError, RefreshResult, RegisterError, RegisterRequest, ResetPasswordError,
    ResetPasswordRequest, VerifyEmailError, decode_session_secret, verify_access_token,
};
pub use domain::{Email, EmailError, Password, PasswordError};
