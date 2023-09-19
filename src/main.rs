use std::time::Duration;

use axum::{extract::Path, response::IntoResponse, routing::get, Router, Server};
use color_eyre::{config::HookBuilder, Report};
use tokio::signal;
use tower::limit::GlobalConcurrencyLimitLayer;
use tower_http::{services::ServeFile, timeout::TimeoutLayer};
use tracing::{debug, error, info, instrument, trace, warn};
use tracing_error::ErrorLayer;
use tracing_subscriber::{fmt, prelude::__tracing_subscriber_SubscriberExt, EnvFilter};

pub type EyreResult<T> = Result<T, Report>;

#[tokio::main]
async fn main() -> EyreResult<()> {
    setup()?;

    // Tracing examples
    error!("Hello!");
    warn!("Hello!");
    info!("Hello!");
    debug!("Hello!");
    trace!("Hello!");

    let app = Router::new()
        .layer(GlobalConcurrencyLimitLayer::new(64))
        .layer(TimeoutLayer::new(Duration::from_secs(30)))
        .route("/", get(root))
        .route("/panic", get(panic))
        .route("/factorial/:num", get(factorial))
        .route_service("/cargo", ServeFile::new("Cargo.toml"));

    Server::bind(&"0.0.0.0:3000".parse()?)
        .serve(app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    opentelemetry::global::shutdown_tracer_provider();

    Ok(())
}

#[instrument]
async fn root() -> impl IntoResponse {
    info!("Hello Axum!");

    "Hello, Axum!"
}

#[instrument]
async fn panic() {
    panic!("Everything is on fire!");
}

#[instrument]
async fn factorial(Path(count): Path<u32>) -> impl IntoResponse {
    let mut total = 1;
    let mut multiplier = count;

    factorial_step(&mut total, &mut multiplier);

    total.to_string()
}

#[instrument]
fn factorial_step(total: &mut u32, multiplier: &mut u32) {
    *total *= *multiplier;
    *multiplier -= 1;

    if *multiplier != 0 {
        factorial_step(total, multiplier);
    }
}

fn setup() -> EyreResult<()> {
    HookBuilder::new()
        .panic_section(
            "    - Reporting this issue to the repository
    - If a developer, review Jaeger traces and provide extra information to issue submissions",
        )
        .issue_url(concat!(env!("CARGO_PKG_REPOSITORY"), "/issues/new"))
        .install()?;

    opentelemetry::global::set_text_map_propagator(opentelemetry_jaeger::Propagator::new());

    let tracer = opentelemetry_jaeger::new_agent_pipeline()
        .with_service_name("axum-server")
        .install_simple()?;
    let opentelemetry = tracing_opentelemetry::layer().with_tracer(tracer);

    let subscriber = tracing_subscriber::registry()
        .with(opentelemetry)
        .with(fmt::Layer::default())
        .with(ErrorLayer::default())
        .with(EnvFilter::from_default_env());

    tracing::subscriber::set_global_default(subscriber)?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("Application stop signal recieved, starting graceful shutdown");
}
