use std::sync::Arc;

use argon2::Argon2;
use argon2::password_hash::PasswordHasher;
use kernel::{UserId, WorkspaceId};
use platform::{Clock, JobQueue};
use rand::RngExt;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use time::Duration as TimeDuration;
use uuid::Uuid;

use crate::domain::{Email, EmailError, Password, PasswordError};
use crate::infra;

const VERIFY_EMAIL_TTL_HOURS: i64 = 24;

#[derive(Debug, thiserror::Error)]
pub enum RegisterError {
    #[error(transparent)]
    InvalidEmail(#[from] EmailError),

    #[error(transparent)]
    InvalidPassword(#[from] PasswordError),

    #[error("failed to hash password")]
    Hash,

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
}

pub struct IdentityService {
    pool: PgPool,
    queue: JobQueue,
    clock: Arc<dyn Clock>,
    public_base_url: String,
}

impl IdentityService {
    pub fn new(
        pool: PgPool,
        queue: JobQueue,
        clock: Arc<dyn Clock>,
        public_base_url: String,
    ) -> Self {
        Self {
            pool,
            queue,
            clock,
            public_base_url,
        }
    }

    /// Registers a user and their personal workspace in one transaction (US-01), then enqueues
    /// a verification email. Deliberately returns the same `Ok(())` whether this was a fresh
    /// registration or the email was already taken -- no account enumeration (US-01).
    #[tracing::instrument(skip_all)]
    pub async fn register(&self, request: RegisterRequest) -> Result<(), RegisterError> {
        let email = Email::parse(&request.email)?;
        let password = Password::parse(&request.password)?;

        let mut tx = self.pool.begin().await?;

        if infra::user_exists_by_email(&mut tx, email.as_str()).await? {
            tx.rollback().await?;
            return Ok(());
        }

        let password_hash = hash_password(password.as_str()).map_err(|_| RegisterError::Hash)?;

        let user_id = UserId::new_v7();
        infra::insert_user(&mut tx, user_id, email.as_str(), &request.display_name).await?;
        infra::insert_credential(&mut tx, user_id, &password_hash).await?;

        let workspace_id = WorkspaceId::new_v7();
        let workspace_name = format!("{}'s workspace", request.display_name);
        infra::insert_personal_workspace(&mut tx, workspace_id, &workspace_name).await?;
        infra::insert_owner_membership(&mut tx, workspace_id, user_id).await?;

        let raw_token = generate_token();
        let token_hash = Sha256::digest(raw_token.as_bytes()).to_vec();
        let expires_at = self.clock.now() + TimeDuration::hours(VERIFY_EMAIL_TTL_HOURS);
        infra::insert_email_token(
            &mut tx,
            Uuid::now_v7(),
            user_id,
            &token_hash,
            "verify_email",
            expires_at,
        )
        .await?;

        tx.commit().await?;

        self.enqueue_verification_email(email.as_str(), &raw_token)
            .await;

        Ok(())
    }

    async fn enqueue_verification_email(&self, to: &str, raw_token: &str) {
        let link = format!("{}/verify-email?token={raw_token}", self.public_base_url);
        let payload = serde_json::json!({
            "to": to,
            "subject": "Verify your Sintade account",
            "body": format!("Click to verify your email: {link}"),
        });
        if let Err(error) = self.queue.enqueue("SendEmail", payload).await {
            tracing::error!(%error, "failed to enqueue verification email");
        }
    }
}

fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    // Argon2::default() is argon2id, m=19 MiB, t=2, p=1 -- exactly docs/design.md's spec
    // ("argon2id, m=19 MiB, t=2, p=1 (OWASP baseline)"); hash_password() generates its own
    // random salt internally.
    let argon2 = Argon2::default();
    Ok(argon2.hash_password(password.as_bytes())?.to_string())
}

fn generate_token() -> String {
    let bytes: [u8; 32] = rand::rng().random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
