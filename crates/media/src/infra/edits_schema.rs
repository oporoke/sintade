//! The `edits` table's own rules (migration `media_edits`): versions are unique per recording, an
//! EDL is a non-empty array, and deleting a recording deletes its edits.

use sqlx::PgPool;
use uuid::Uuid;

async fn seed(pool: &PgPool) -> (Uuid, Uuid, Uuid) {
    let (user, workspace, recording) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    sqlx::query!(
        "INSERT INTO users (id, email, display_name) VALUES ($1, $2, 'E')",
        user,
        format!("edits-{user}@example.com")
    )
    .execute(pool)
    .await
    .expect("user");
    sqlx::query!(
        "INSERT INTO workspaces (id, name) VALUES ($1, 'E')",
        workspace
    )
    .execute(pool)
    .await
    .expect("workspace");
    sqlx::query!(
        "INSERT INTO recordings (id, workspace_id, owner_id, title, state)
         VALUES ($1, $2, $3, 'E', 'ready')",
        recording,
        workspace,
        user
    )
    .execute(pool)
    .await
    .expect("recording");
    (user, workspace, recording)
}

async fn insert(
    pool: &PgPool,
    ids: (Uuid, Uuid, Uuid),
    version: i32,
    edl: serde_json::Value,
) -> Result<(), sqlx::Error> {
    let (user, workspace, recording) = ids;
    sqlx::query!(
        "INSERT INTO edits (id, workspace_id, recording_id, version, edl, source_duration_ms, created_by)
         VALUES ($1, $2, $3, $4, $5, 60000, $6)",
        Uuid::now_v7(),
        workspace,
        recording,
        version,
        edl,
        user
    )
    .execute(pool)
    .await
    .map(|_| ())
}

#[sqlx::test(migrations = "../../migrations")]
async fn versions_are_unique_per_recording_and_start_at_one(pool: PgPool) {
    let ids = seed(&pool).await;
    let edl = serde_json::json!([[0, 5000]]);
    insert(&pool, ids, 1, edl.clone()).await.expect("first");
    insert(&pool, ids, 2, edl.clone()).await.expect("second");
    assert!(
        insert(&pool, ids, 2, edl.clone()).await.is_err(),
        "same version twice"
    );
    assert!(
        insert(&pool, ids, 0, edl).await.is_err(),
        "versions start at 1"
    );
    // Another recording has its own numbering.
    let other = seed(&pool).await;
    insert(&pool, other, 1, serde_json::json!([[0, 5000]]))
        .await
        .expect("another recording's first");
}

#[sqlx::test(migrations = "../../migrations")]
async fn an_edl_is_a_non_empty_array(pool: PgPool) {
    let ids = seed(&pool).await;
    for bad in [
        serde_json::json!([]),
        serde_json::json!({"0": [0, 5000]}),
        serde_json::json!("x"),
    ] {
        assert!(insert(&pool, ids, 1, bad).await.is_err());
    }
    insert(&pool, ids, 1, serde_json::json!([[0, 5000]]))
        .await
        .expect("a real EDL");
}

#[sqlx::test(migrations = "../../migrations")]
async fn deleting_a_recording_deletes_its_edits(pool: PgPool) {
    let ids = seed(&pool).await;
    insert(&pool, ids, 1, serde_json::json!([[0, 5000]]))
        .await
        .expect("edit");
    sqlx::query!("DELETE FROM recordings WHERE id = $1", ids.2)
        .execute(&pool)
        .await
        .expect("delete");
    let left = sqlx::query_scalar!("SELECT count(*) FROM edits WHERE recording_id = $1", ids.2)
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(left, Some(0));
}
