/// What a workspace's plan allows (docs/design.md §4: computed by `billing`; other modules read
/// these limits, never plan names).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entitlements {
    /// Recordings the workspace may hold (not counting abandoned or trashed ones).
    pub max_recordings: u32,
    /// Longest take, pauses excluded.
    pub max_duration_ms: u32,
    /// Tallest recording resolution, in pixels (1080 = 1080p).
    pub max_resolution: u32,
    pub seats: u32,
}
