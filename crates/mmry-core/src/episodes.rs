// Episodes: append-only log of (query, returned_ids, used_ids, agent_ctx, ts).
//
// One row per `mmry search` call; closed by a subsequent `mmry add --using <ids>`
// in the same session. All scoring signals (explicit usage, implicit positive
// via same-session closure, implicit negative via reformulation, co-return lift,
// drift decay) are derived as pure SQL aggregates over this table at retrieval
// time. No state, no derived columns — storage stays dumb.

use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use sqlx::Row;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::agent_ctx::CtxIndexKeys;

/// One search interaction. `used_ids` and `closed_at` are filled in when the
/// agent later writes a memory citing the returned ids (or the episode times
/// out unresolved).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Episode {
    pub id: Uuid,
    pub query: String,
    pub returned_ids: Vec<Uuid>,
    pub used_ids: Option<Vec<Uuid>>,
    pub result: Option<String>,
    pub workspace_id: Option<String>,
    pub platform_session_id: Option<String>,
    pub harness_session_id: Option<String>,
    pub ts: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
}

/// Append a new episode for a search call. Best-effort: the caller should
/// ignore errors so a failed write never breaks the search response.
pub async fn record_episode(
    pool: &SqlitePool,
    query: &str,
    returned_ids: &[Uuid],
    ctx: CtxIndexKeys<'_>,
) -> crate::Result<Uuid> {
    let id = Uuid::new_v4();
    let returned_json =
        serde_json::to_string(&returned_ids.iter().map(Uuid::to_string).collect::<Vec<_>>())?;

    sqlx::query(
        r#"
        INSERT INTO episodes (
            id, query, returned_ids, workspace_id, platform_session_id,
            harness_session_id, ts
        )
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(id.to_string())
    .bind(query)
    .bind(returned_json)
    .bind(ctx.workspace_id)
    .bind(ctx.platform_session_id)
    .bind(ctx.harness_session_id)
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await?;

    Ok(id)
}

/// Close an episode with the ids that the agent actually cited. Bumps
/// `helpful_count` on each cited memory in the same call so downstream
/// scoring sees the feedback immediately.
pub async fn close_episode(
    pool: &SqlitePool,
    episode_id: Uuid,
    used_ids: &[Uuid],
    result: Option<&str>,
) -> crate::Result<()> {
    let used_json =
        serde_json::to_string(&used_ids.iter().map(Uuid::to_string).collect::<Vec<_>>())?;
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        r#"
        UPDATE episodes
        SET used_ids = ?, result = ?, closed_at = ?
        WHERE id = ? AND closed_at IS NULL
        "#,
    )
    .bind(used_json)
    .bind(result)
    .bind(&now)
    .bind(episode_id.to_string())
    .execute(pool)
    .await?;

    for id in used_ids {
        sqlx::query(
            r#"
            UPDATE memories
            SET helpful_count = COALESCE(helpful_count, 0) + 1, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&now)
        .bind(id.to_string())
        .execute(pool)
        .await?;
    }

    Ok(())
}

/// Find the most recent open episode for the current agent session, within
/// `max_age_seconds`. Used by `mmry add --using` when the caller did not pass
/// an explicit `--episode <id>`.
///
/// Session match preference (most specific first): platform_session_id,
/// harness_session_id, workspace_id. If no key is available the lookup is
/// skipped and `Ok(None)` is returned.
pub async fn find_latest_open_episode(
    pool: &SqlitePool,
    ctx: CtxIndexKeys<'_>,
    max_age_seconds: i64,
) -> crate::Result<Option<Uuid>> {
    let cutoff = (Utc::now() - chrono::Duration::seconds(max_age_seconds)).to_rfc3339();

    let (column, value) = if let Some(v) = ctx.platform_session_id {
        ("platform_session_id", v)
    } else if let Some(v) = ctx.harness_session_id {
        ("harness_session_id", v)
    } else if let Some(v) = ctx.workspace_id {
        ("workspace_id", v)
    } else {
        return Ok(None);
    };

    let sql = format!(
        r#"
        SELECT id FROM episodes
        WHERE {column} = ? AND closed_at IS NULL AND ts >= ?
        ORDER BY ts DESC
        LIMIT 1
        "#
    );

    let row = sqlx::query(&sql)
        .bind(value)
        .bind(&cutoff)
        .fetch_optional(pool)
        .await?;

    match row {
        Some(r) => {
            let raw: String = r.try_get("id")?;
            Ok(Some(Uuid::parse_str(&raw).map_err(|e| {
                crate::Error::InvalidInput(format!("Invalid episode id '{raw}': {e}"))
            })?))
        }
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::schema;

    async fn pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(schema::INIT_SQL).execute(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn record_and_close_episode_round_trip() {
        let pool = pool().await;
        let m1 = Uuid::new_v4();
        let m2 = Uuid::new_v4();

        // Seed two memories so close_episode's UPDATE has rows to bump.
        for id in [m1, m2] {
            sqlx::query(
                "INSERT INTO memories (id, type, content, helpful_count) VALUES (?, 'note', 'x', 0)",
            )
            .bind(id.to_string())
            .execute(&pool)
            .await
            .unwrap();
        }

        let ctx = CtxIndexKeys {
            workspace_id: Some("ws_a"),
            platform_session_id: Some("sess_a"),
            harness_session_id: None,
        };
        let ep = record_episode(&pool, "how to foo", &[m1, m2], ctx)
            .await
            .unwrap();

        close_episode(&pool, ep, &[m1], Some("succeeded"))
            .await
            .unwrap();

        let row = sqlx::query("SELECT used_ids, result, closed_at FROM episodes WHERE id = ?")
            .bind(ep.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
        let used_raw: String = row.try_get("used_ids").unwrap();
        assert!(used_raw.contains(&m1.to_string()));
        let result: String = row.try_get("result").unwrap();
        assert_eq!(result, "succeeded");
        let closed: Option<String> = row.try_get("closed_at").unwrap();
        assert!(closed.is_some());

        let bumped: i64 = sqlx::query_scalar("SELECT helpful_count FROM memories WHERE id = ?")
            .bind(m1.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(bumped, 1);
        let untouched: i64 = sqlx::query_scalar("SELECT helpful_count FROM memories WHERE id = ?")
            .bind(m2.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(untouched, 0);
    }

    #[tokio::test]
    async fn find_latest_open_episode_matches_session_and_skips_closed() {
        let pool = pool().await;
        let ctx = CtxIndexKeys {
            workspace_id: Some("ws_a"),
            platform_session_id: Some("sess_a"),
            harness_session_id: None,
        };

        let older = record_episode(&pool, "q1", &[], ctx).await.unwrap();
        let newer = record_episode(&pool, "q2", &[], ctx).await.unwrap();

        let found = find_latest_open_episode(&pool, ctx, 3600).await.unwrap();
        assert_eq!(found, Some(newer));

        close_episode(&pool, newer, &[], Some("succeeded"))
            .await
            .unwrap();
        let found = find_latest_open_episode(&pool, ctx, 3600).await.unwrap();
        assert_eq!(found, Some(older));

        close_episode(&pool, older, &[], Some("succeeded"))
            .await
            .unwrap();
        let found = find_latest_open_episode(&pool, ctx, 3600).await.unwrap();
        assert_eq!(found, None);
    }

    #[tokio::test]
    async fn find_latest_open_episode_returns_none_without_session_keys() {
        let pool = pool().await;
        let empty = CtxIndexKeys::default();
        assert!(find_latest_open_episode(&pool, empty, 3600)
            .await
            .unwrap()
            .is_none());
    }
}
