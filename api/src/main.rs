//! Canyon Oaks API: layers, features, attachments, search and homeowner issues.
mod config;
mod email;
mod error;
mod issues;
mod layers;
mod ratelimit;

use std::{net::SocketAddr, sync::Arc};

use axum::{extract::DefaultBodyLimit, routing::get, Router};
use sqlx::postgres::PgPoolOptions;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::PgPool,
    pub cfg: Arc<config::Config>,
    pub limiter: Arc<ratelimit::RateLimiter>,
}

#[tokio::main]
async fn main() -> anyhow_lite::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        std::process::exit(if healthcheck() { 0 } else { 1 });
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "canyon_oaks_api=info,tower_http=info".into()),
        )
        .init();

    let cfg = config::Config::from_env()?;
    let db = connect_with_retry(&cfg.database_url).await?;
    sqlx::migrate!("./migrations").run(&db).await?;
    match sqlx::query_scalar::<_, i32>("SELECT app.import_legacy_issues()")
        .fetch_one(&db)
        .await
    {
        Ok(n) if n > 0 => tracing::info!("imported {n} legacy Survey123 reports"),
        Ok(_) => {}
        Err(e) => tracing::warn!("legacy import skipped: {e}"),
    }
    tokio::fs::create_dir_all(&cfg.uploads_dir).await.ok();
    if cfg.email.enabled {
        tracing::info!("email notifications ENABLED");
    } else {
        tracing::info!("email notifications disabled (EMAIL_ENABLED=false)");
    }

    let state = AppState {
        db,
        limiter: Arc::new(ratelimit::RateLimiter::new(cfg.rate_limit_per_hour)),
        cfg: Arc::new(cfg),
    };
    let bind: SocketAddr = state.cfg.bind.parse()?;

    let app = Router::new()
        .route("/api/health", get(layers::health))
        .route("/api/layers", get(layers::list_layers))
        .route("/api/features/{layer}", get(layers::features))
        .route("/api/search", get(layers::search))
        .route("/api/stats", get(layers::stats))
        .route("/api/attachments/{layer}/{objectid}", get(layers::list_attachments))
        .route("/api/attachments/{layer}/{objectid}/{attachment_id}", get(layers::attachment_file))
        .route("/api/issues", get(issues::list).post(issues::create))
        .route("/api/issues/options", get(issues::options))
        .route(
            "/api/issues/{ticket}",
            get(issues::get_one).patch(issues::update).delete(issues::delete),
        )
        .route("/api/issues/{ticket}/photos/{photo_id}", get(issues::photo))
        .layer(DefaultBodyLimit::max(40 * 1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    tracing::info!("listening on {bind}");
    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

async fn connect_with_retry(url: &str) -> anyhow_lite::Result<sqlx::PgPool> {
    let mut wait = 1;
    loop {
        match PgPoolOptions::new().max_connections(10).connect(url).await {
            Ok(p) => return Ok(p),
            Err(e) if wait < 60 => {
                tracing::warn!("database not ready ({e}); retrying in {wait}s");
                tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                wait *= 2;
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// `canyon-oaks-api healthcheck`: used by the container HEALTHCHECK (no curl in the image).
fn healthcheck() -> bool {
    use std::io::{Read, Write};
    let port = std::env::var("BIND").ok().and_then(|b| b.rsplit(':').next().map(String::from)).unwrap_or_else(|| "3000".into());
    let Ok(mut s) = std::net::TcpStream::connect(format!("127.0.0.1:{port}")) else { return false };
    let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    if s.write_all(b"GET /api/health HTTP/1.0\r\nHost: localhost\r\n\r\n").is_err() { return false; }
    let mut buf = String::new();
    let _ = s.read_to_string(&mut buf);
    buf.starts_with("HTTP/1.0 200") || buf.starts_with("HTTP/1.1 200")
}

/// Tiny boxed-error alias so main can use `?` on anything.
mod anyhow_lite {
    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
}
