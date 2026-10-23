//! Read API over the DYNEXUS index. The web app reads lists, history and aggregates here
//! instead of issuing one view-function RPC per token/pool; wallet balances and
//! transaction submission still go to the chain directly.

mod routes;

use anyhow::{Context, Result};
use diesel::{ConnectionError, ConnectionResult};
use diesel_async::{
    AsyncPgConnection,
    pooled_connection::{AsyncDieselConnectionManager, ManagerConfig, bb8::Pool},
};
use futures_util::{FutureExt, future::BoxFuture};
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
    // API_ADDR wins; otherwise PORT (set by hosts like Render) on all interfaces.
    let addr: SocketAddr = match std::env::var("API_ADDR") {
        Ok(addr) => addr.parse().context("API_ADDR must be host:port")?,
        Err(_) => {
            let port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
            format!("0.0.0.0:{port}")
                .parse()
                .context("PORT must be a port number")?
        }
    };

    let mut config = ManagerConfig::<AsyncPgConnection>::default();
    config.custom_setup = Box::new(establish_connection);
    let manager = AsyncDieselConnectionManager::<AsyncPgConnection>::new_with_config(database_url, config);
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

/// Connects with TLS available, verified against the system roots. `sslmode` in the URL
/// decides whether it is used: `require` for hosted Postgres (Neon), and the default
/// `prefer` still works against a local server without TLS.
fn establish_connection(url: &str) -> BoxFuture<'_, ConnectionResult<AsyncPgConnection>> {
    async move {
        let tls = native_tls::TlsConnector::new()
            .map_err(|e| ConnectionError::BadConnection(format!("TLS setup: {e}")))?;
        let (client, connection) =
            tokio_postgres::connect(url, postgres_native_tls::MakeTlsConnector::new(tls))
                .await
                .map_err(|e| ConnectionError::BadConnection(e.to_string()))?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!(error = %e, "postgres connection error");
            }
        });
        AsyncPgConnection::try_from(client).await
    }
    .boxed()
}
