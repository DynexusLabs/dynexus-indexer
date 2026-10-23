//! Read API over the DYNEXUS index. The web app reads lists, history and aggregates here
//! instead of issuing one view-function RPC per token/pool; wallet balances and
//! transaction submission still go to the chain directly.

mod routes;

use anyhow::{Context, Result};
use diesel_async::{
    AsyncPgConnection,
    pooled_connection::{AsyncDieselConnectionManager, bb8::Pool},
};
use std::net::SocketAddr;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::EnvFilter;

pub type DbPool = Pool<AsyncPgConnection>;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
    let addr: SocketAddr = std::env::var("API_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".into())
        .parse()
        .context("API_ADDR must be host:port")?;

    let manager = AsyncDieselConnectionManager::<AsyncPgConnection>::new(database_url);
    let pool = Pool::builder().max_size(16).build(manager).await?;

    let cors = match std::env::var("CORS_ORIGIN") {
        Ok(origin) => CorsLayer::new()
            .allow_origin(origin.parse::<axum::http::HeaderValue>()?)
            .allow_methods([axum::http::Method::GET]),
        Err(_) => CorsLayer::permissive(),
    };
    let app = routes::router(pool).layer(cors).layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "dynexus-api listening");
    axum::serve(listener, app).await?;
    Ok(())
}
