use kernel::Entitlements;

/// The free tier (docs/design.md §13 decision log): 50 recordings, 10 minutes each, up to 1080p,
/// one seat.
pub const FREE_TIER: Entitlements = Entitlements {
    max_recordings: 50,
    max_duration_ms: 10 * 60 * 1000,
    max_resolution: 1080,
    seats: 1,
};
