use axum::{
    routing::{get, post},
    extract::{Path, State, Query, WebSocketUpgrade},
    response::IntoResponse,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::{broadcast, RwLock};
use tracing::{info, error};
use sqlx::postgres::PgPoolOptions;

use engine::{
    block::{BlockBuilder, BlockNumber, BatchId},
    commit::BlakePoseidonStub,
    db::PgDb,
    types::*,
};

#[derive(Clone)]
struct AppState {
    db: PgDb,
    pool: sqlx::Pool<sqlx::Postgres>,
    hasher: BlakePoseidonStub,
    // pubsub
    tx_blocks: broadcast::Sender<engine::block::BlockHeader>,
    tx_fills:  broadcast::Sender<FillDraft>,
    tx_books:  broadcast::Sender<(PairId, TopOfBook)>,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct TopOfBook {
    best_bid: Option<(u64 /*price*/, u64 /*qty*/ )>,
    best_ask: Option<(u64, u64)>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    let dsn = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL is required");

    let pool = PgPoolOptions::new().max_connections(10).connect(&dsn).await?;
    let db = PgDb::new(pool.clone());
    let hasher = BlakePoseidonStub;

    let (tx_blocks, _) = broadcast::channel(1024);
    let (tx_fills,  _) = broadcast::channel(4096);
    let (tx_books,  _) = broadcast::channel(2048);

    let state = AppState{ db, pool, hasher, tx_blocks, tx_fills, tx_books };

    // background batcher
    tokio::spawn(batch_loop(state.clone()));

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/v1/markets", get(get_markets))
        .route("/v1/orderbook/:pair_id", get(get_orderbook))
        .route("/v1/fills", get(get_fills))
        .route("/v1/blocks/:block_number", get(get_block))
        .route("/v1/orders", post(post_order))
        .route("/ws", get(ws_handler))
        .with_state(state);

    let addr: SocketAddr = "0.0.0.0:8080".parse().unwrap();
    info!("sequencer listening on {}", addr);
    axum::Server::bind(&addr).serve(app.into_make_service()).await?;
    Ok(())
}
async fn batch_loop(state: AppState) {
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
    let mut next_block_number = state.db.get_latest_block_number().await.unwrap_or(0) + 1;
    loop {
        interval.tick().await;
        match state.db.fetch_pending_orders().await {
            Ok(orders) if !orders.is_empty() => {
                info!("batching {} orders into block {}", orders.len(), next_block_number);
                let mut builder = BlockBuilder::new(next_block_number, &state.hasher);
                for order in orders {
                    if let Err(e) = builder.add_order(order) {
                        error!("failed to add order to block {}: {}", next_block_number, e);
                    }
                }
                let (block, fills, top_of_book) = builder.finalize();
                if let Err(e) = state.db.commit_block(block.clone(), fills.clone()).await {
                    error!("failed to commit block {}: {}", next_block_number, e);
                    continue;
                }
                // publish events
                if let Err(e) = state.tx_blocks.send(block.header.clone()) {
                    error!("failed to publish block {}: {}", next_block_number, e);
                }
                for fill in fills.iter() {
                    if let Err(e) = state.tx_fills.send(fill.clone()) {
                        error!("failed to publish fill in block {}: {}", next_block_number, e);
                    }
                }
                for (pair_id, tob) in top_of_book.iter() {
                    if let Err(e) = state.tx_books.send((*pair_id, tob.clone())) {
                        error!("failed to publish top-of-book for pair {} in block {}: {}", pair_id, next_block_number, e);
                    }
                }
                next_block_number += 1;
            },
            Ok(_) => { /* no pending orders */ },
            Err(e) => {
                error!("failed to fetch pending orders: {}", e);
            }
        }
    }
}
