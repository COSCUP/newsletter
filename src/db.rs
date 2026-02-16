use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

pub async fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await
}

pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::Error> {
    let migration_001 = include_str!("../migrations/001_initial.sql");
    sqlx::raw_sql(migration_001).execute(pool).await?;

    let migration_002 = include_str!("../migrations/002_newsletters.sql");
    sqlx::raw_sql(migration_002).execute(pool).await?;

    let migration_003 = include_str!("../migrations/003_click_url.sql");
    sqlx::raw_sql(migration_003).execute(pool).await?;

    let migration_004 = include_str!("../migrations/004_template_management.sql");
    sqlx::raw_sql(migration_004).execute(pool).await?;

    let migration_005 = include_str!("../migrations/005_bounced_at.sql");
    sqlx::raw_sql(migration_005).execute(pool).await?;

    let migration_006 = include_str!("../migrations/006_subscribe_email_log.sql");
    sqlx::raw_sql(migration_006).execute(pool).await?;

    let migration_007 = include_str!("../migrations/007_update_default_template_logo.sql");
    sqlx::raw_sql(migration_007).execute(pool).await?;

    let migration_008 = include_str!("../migrations/008_unsubscribe_events.sql");
    sqlx::raw_sql(migration_008).execute(pool).await?;

    let migration_009 = include_str!("../migrations/009_update_template_web_url.sql");
    sqlx::raw_sql(migration_009).execute(pool).await?;

    let migration_010 = include_str!("../migrations/010_admins.sql");
    sqlx::raw_sql(migration_010).execute(pool).await?;

    let migration_011 = include_str!("../migrations/011_audit_log.sql");
    sqlx::raw_sql(migration_011).execute(pool).await?;

    Ok(())
}

pub async fn sync_seed_admins(pool: &PgPool, admin_emails: &[String]) -> Result<(), sqlx::Error> {
    for email in admin_emails {
        sqlx::query(
            "INSERT INTO admins (email, added_by) VALUES ($1, 'seed') ON CONFLICT (email) DO NOTHING",
        )
        .bind(email)
        .execute(pool)
        .await?;
    }
    Ok(())
}
