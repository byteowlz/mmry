pub mod operations;
pub mod schema;

use sqlx::sqlite::SqlitePool;
use std::path::Path;

pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn new(database_url: &str) -> crate::Result<Self> {
        let pool = SqlitePool::connect(database_url).await?;
        Ok(Self { pool })
    }

    pub async fn init(path: &Path) -> crate::Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let database_url = format!("sqlite://{}?mode=rwc", path.display());
        let pool = SqlitePool::connect(&database_url).await?;

        // Initialize schema
        sqlx::query(schema::INIT_SQL).execute(&pool).await?;

        // Apply schema migrations
        Self::apply_schema_updates(&pool).await?;

        Ok(Self { pool })
    }

    async fn apply_schema_updates(pool: &SqlitePool) -> crate::Result<()> {
        // Check if sparse_embedding column exists, add if not
        let sparse_column_exists: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('memories') WHERE name='sparse_embedding'",
        )
        .fetch_one(pool)
        .await?;

        if !sparse_column_exists {
            tracing::info!("Adding sparse_embedding column to memories table...");
            sqlx::query("ALTER TABLE memories ADD COLUMN sparse_embedding BLOB")
                .execute(pool)
                .await?;
            tracing::info!("sparse_embedding column added");
        }

        // Check if we need to rename namespace to category
        let namespace_exists: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('memories') WHERE name='namespace'",
        )
        .fetch_one(pool)
        .await?;

        let category_exists: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('memories') WHERE name='category'",
        )
        .fetch_one(pool)
        .await?;

        if namespace_exists && !category_exists {
            tracing::info!("Migrating 'namespace' column to 'category'...");
            sqlx::query("ALTER TABLE memories RENAME COLUMN namespace TO category")
                .execute(pool)
                .await?;

            // Drop old index and create new one
            sqlx::query("DROP INDEX IF EXISTS idx_memories_namespace")
                .execute(pool)
                .await?;
            sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_category ON memories(category)")
                .execute(pool)
                .await?;

            tracing::info!("Column renamed from 'namespace' to 'category'");
        }

        // Check if tags column exists, add if not
        let tags_column_exists: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('memories') WHERE name='tags'",
        )
        .fetch_one(pool)
        .await?;

        if !tags_column_exists {
            tracing::info!("Adding tags column to memories table...");
            sqlx::query("ALTER TABLE memories ADD COLUMN tags JSON DEFAULT '[]'")
                .execute(pool)
                .await?;
            tracing::info!("tags column added");
        }

        Ok(())
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn close(self) {
        self.pool.close().await;
    }
}
