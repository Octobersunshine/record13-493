mod db;
mod handlers;
mod models;
mod routes;
mod storage;

use std::net::SocketAddr;

use anyhow::Result;
use tokio::net::TcpListener;
use tracing::{info, level_filters::LevelFilter};
use tracing_subscriber::EnvFilter;

use crate::db::init_db;
use crate::routes::create_router;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .with_target(false)
        .init();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "sqlite:cold_chain.db".to_string());

    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse::<u16>()
        .unwrap_or(3000);

    info!("正在初始化数据库...");
    let pool = init_db(&database_url).await?;
    info!("数据库初始化完成");

    let app = create_router(pool);

    let addr: SocketAddr = format!("0.0.0.0:{}", port).parse()?;
    info!("冷链温度监控服务启动，监听地址: {}", addr);

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
