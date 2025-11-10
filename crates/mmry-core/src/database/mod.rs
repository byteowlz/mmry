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
            tracing::info!("Schema update completed");
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
