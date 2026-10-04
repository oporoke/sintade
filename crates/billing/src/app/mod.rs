use kernel::{Entitlements, WorkspaceId};

use crate::domain::FREE_TIER;

/// Entitlements per workspace. An MVP stub (KI-006): every workspace is on the free tier until
/// V1 adds plans, subscriptions and mobile-money payments behind this same interface.
#[derive(Debug, Default)]
pub struct BillingService;

impl BillingService {
    pub fn new() -> Self {
        Self
    }

    /// Async because V1 reads plans and subscriptions (cached 60 s, §11).
    #[tracing::instrument(skip_all, fields(workspace_id = %workspace_id))]
    pub async fn entitlements(&self, workspace_id: WorkspaceId) -> Entitlements {
        FREE_TIER
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn every_workspace_gets_the_free_tier() {
        let entitlements = BillingService::new()
            .entitlements(WorkspaceId::new_v7())
            .await;
        assert_eq!(entitlements.max_recordings, 50);
        assert_eq!(entitlements.max_duration_ms, 600_000);
        assert_eq!(entitlements.max_resolution, 1080);
    }
}
