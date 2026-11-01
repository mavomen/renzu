//! Database module for the gateway HTTP service.
//!
//! Provides a PostgreSQL connection pool via [`sqlx`], a health-check query,
//! and a minimal database-backed operation. All database operations are
//! instrumented with tracing spans so they appear in distributed traces.

use sqlx::postgres::PgPoolOptions;
use sqlx::Pool;
use sqlx::Postgres;
use std::time::Duration;
use tracing::instrument;

/// Type alias for the PostgreSQL connection pool.
pub type DbPool = Pool<Postgres>;

/// Initialize the PostgreSQL connection pool.
///
/// Reads `database_url` from configuration. Configures a connection timeout
/// so that startup does not block indefinitely when the database is
/// unavailable. Returns the pool; callers are responsible for closing it on
/// shutdown.
pub async fn init_db(database_url: &str) -> Result<DbPool, sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await?;
    Ok(pool)
}

/// Run a simple connectivity check against the database.
///
/// Executes `SELECT 1` and returns `Ok(())` if the query succeeds. Used by
/// the readiness endpoint to determine whether the database is reachable.
#[instrument(name = "db.health_check", skip_all, fields(db.system = "postgresql"))]
pub async fn health_check(pool: &DbPool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1").execute(pool).await?;
    Ok(())
}

/// Record a minimal database-backed operation.
///
/// Inserts a row into a `health_checks` table (created if it does not exist)
/// and returns the current row count. This demonstrates a write followed by a
/// read against PostgreSQL, with both operations traced.
#[instrument(name = "db.record_health_check", skip_all, fields(db.system = "postgresql", db.operation = "insert"))]
pub async fn record_health_check(pool: &DbPool) -> Result<i64, sqlx::Error> {
    // Ensure the table exists. This is a one-time setup per deployment; in a
    // production system this would be handled by a migration tool.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS health_checks (id SERIAL PRIMARY KEY, checked_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
    )
    .execute(pool)
    .await?;

    sqlx::query("INSERT INTO health_checks DEFAULT VALUES")
        .execute(pool)
        .await?;

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM health_checks")
        .fetch_one(pool)
        .await?;

    Ok(count)
}
