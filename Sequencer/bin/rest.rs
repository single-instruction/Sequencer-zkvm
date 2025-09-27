use axum::{extract::{Path, State, Query}, Json};
use serde::{Serialize, Deserialize};
use super::*;

#[derive(Serialize)]
struct MarketDTO {
    pair_id: u32,
    price_tick: u64,
    size_step: u64,
    maker_bps: u16,
    taker_bps: u16,
    status: u8,
}
pub async fn get_markets(State(state): State<AppState>) -> Json<Vec<MarketDTO>> {
    // reuse engine’s PgDb transactional read
    let mut tx = state.db.begin_repeatable_read().await.unwrap();
    let mkts = tx.load_active_markets().await.unwrap();
    let out = mkts.into_iter().map(|m| MarketDTO{
        pair_id: m.pair_id.0,
        price_tick: m.price_tick,
        size_step: m.size_step,
        maker_bps: m.maker_bps,
        taker_bps: m.taker_bps,
        status: match m.status {
            MarketStatus::Active=>0, MarketStatus::Paused=>1, MarketStatus::CancelOnly=>2, MarketStatus::Delisted=>3
        }
    }).collect();
    Json(out)
}

#[derive(Deserialize)]
pub struct FillsQuery {
    pair_id: Option<u32>,
    batch_id: Option<u64>,
    limit: Option<i64>,
}

#[derive(Serialize)]
pub struct FillDTO {
    batch_id: u64,
    match_id: u64,
    pair_id: u32,
    price_tick: u64,
    fill_qty: u64,
    time_bucket: u32,
    buyer_pid: String,
    seller_pid: String,
}
pub async fn get_fills(
    State(state): State<AppState>,
    Query(q): Query<FillsQuery>
) -> Json<Vec<FillDTO>> {
    let limit = q.limit.unwrap_or(200).clamp(1, 1000);
    // simple direct SQL via pool for listing
    let rows = if let (Some(pair), Some(batch)) = (q.pair_id, q.batch_id) {
        sqlx::query!(r#"
            SELECT batch_id, match_id, pair_id, price_tick, fill_qty, time_bucket,
                   buyer_pid, seller_pid
            FROM fills
            WHERE pair_id=$1 AND batch_id=$2
            ORDER BY match_id ASC
            LIMIT $3
        "#, pair as i64, batch as i64, limit)
        .fetch_all(&state.pool).await.unwrap()
    } else {
        sqlx::query!(r#"
            SELECT batch_id, match_id, pair_id, price_tick, fill_qty, time_bucket,
                   buyer_pid, seller_pid
            FROM fills
            ORDER BY batch_id DESC, match_id ASC
            LIMIT $1
        "#, limit)
        .fetch_all(&state.pool).await.unwrap()
    };

    let out = rows.into_iter().map(|r| FillDTO{
        batch_id: r.batch_id as u64,
        match_id: r.match_id as u64,
        pair_id: r.pair_id as u32,
        price_tick: r.price_tick as u64,
        fill_qty: r.fill_qty as u64,
        time_bucket: r.time_bucket as u32,
        buyer_pid: hex::encode(r.buyer_pid),
        seller_pid: hex::encode(r.seller_pid),
    }).collect();
    Json(out)
}

#[derive(Serialize)]
pub struct BlockHeaderDTO {
    block_number: u64,
    batch_id: u64,
    parent_state_root: String,
    new_state_root: String,
    markets_root: String,
    orders_commitment: String,
    fills_commitment: String,
    timestamp_ms: u64,
}
pub async fn get_block(
    State(state): State<AppState>,
    Path(block_number): Path<u64>,
) -> Option<Json<BlockHeaderDTO>> {
    let r = sqlx::query!(r#"
        SELECT block_number, batch_id, parent_state_root, new_state_root,
               markets_root, orders_commitment, fills_commitment, timestamp_ms
        FROM batches WHERE block_number=$1
    "#, block_number as i64).fetch_optional(&state.pool).await.ok()??;

    Some(Json(BlockHeaderDTO{
        block_number: r.block_number as u64,
        batch_id: r.batch_id as u64,
        parent_state_root: hex::encode(r.parent_state_root),
        new_state_root: hex::encode(r.new_state_root),
        markets_root: hex::encode(r.markets_root),
        orders_commitment: hex::encode(r.orders_commitment),
        fills_commitment: hex::encode(r.fills_commitment),
        timestamp_ms: r.timestamp_ms as u64,
    }))
}

pub async fn get_orderbook(
    State(state): State<AppState>,
    Path(pair_id): Path<u32>,
) -> Json<TopOfBook> {
    // Quick & dirty: compute from open orders table (for small scale)
    let rows = sqlx::query!(r#"
        SELECT side, price_tick, SUM(remaining) as qty
        FROM orders
        WHERE pair_id=$1 AND remaining>0
        GROUP BY side, price_tick
        ORDER BY price_tick ASC
    "#, pair_id as i64).fetch_all(&state.pool).await.unwrap();

    let mut best_bid: Option<(u64,u64)> = None;
    let mut best_ask: Option<(u64,u64)> = None;
    for r in rows {
        let px = r.price_tick as u64;
        let qty = (r.qty.unwrap_or_default()) as u64;
        if r.side == 0 {
            // bid
            match best_bid { None => best_bid = Some((px, qty)), Some((bp,_)) if px > bp => best_bid = Some((px,qty)), _=>{} }
        } else {
            // ask
            match best_ask { None => best_ask = Some((px, qty)), Some((ap,_)) if px < ap => best_ask = Some((px,qty)), _=>{} }
        }
    }

    Json(TopOfBook{ best_bid, best_ask })
}


#[derive(Deserialize)]
struct SubmitOrderReq {
    pair_id: u32,
    side: u8,                 // 0=Bid,1=Ask
    price_tick: u64,
    amount: u64,
    time_bucket: u32,
    nonce: u64,
    order_hash: String,       // hex of the signed payload hash
    pk_hash: String,          // Poseidon(Ax,Ay,0) hex (private identity hash)
}
#[derive(Serialize)]
struct SubmitOrderRes {
    order_id: u64,
    ingest_seq: u64,
}

pub async fn post_order(
    State(state): State<AppState>,
    Json(req): Json<SubmitOrderReq>,
) -> Result<Json<SubmitOrderRes>, (axum::http::StatusCode, String)> {
    let mut conn = state.pool.acquire().await.map_err(internal)?;
    // Begin tx
    sqlx::query("BEGIN").execute(&mut *conn).await.map_err(internal)?;

    // allocate ingest_seq
    let row = sqlx::query!(
        r#"UPDATE market_counters
           SET next_ingest_seq = next_ingest_seq + 1
           WHERE pair_id = $1
           RETURNING next_ingest_seq - 1 AS ingest_seq"#,
        req.pair_id as i64
    ).fetch_one(&mut *conn).await.map_err(bad)?;

    let ingest_seq = row.ingest_seq.unwrap_or(0) as u64;
    let order_id = uuid::Uuid::new_v4().as_u128() as u64; // or your per-market counter

    let order_hash = hex::decode(req.order_hash).map_err(bad)?;
    let pk_hash    = hex::decode(req.pk_hash).map_err(bad)?;
    if order_hash.len()!=32 || pk_hash.len()!=32 {
        return Err(bad((400, "hash length must be 32 bytes".into())));
    }

    // insert order
    sqlx::query!(
        r#"INSERT INTO orders
           (order_id, order_hash, pair_id, side, price_tick, amount, remaining,
            time_bucket, nonce, ingest_seq, status)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,0)"#,
        order_id as i64, &order_hash, req.pair_id as i64, req.side as i16,
        req.price_tick as i64, req.amount as i64, req.amount as i64,
        req.time_bucket as i32, req.nonce as i64, ingest_seq as i64
    ).execute(&mut *conn).await.map_err(bad)?;

    // private owner mapping
    sqlx::query!(
        r#"INSERT INTO order_owners_private (order_id, pk_hash) VALUES ($1,$2)"#,
        order_id as i64, &pk_hash
    ).execute(&mut *conn).await.map_err(bad)?;

    // commit
    sqlx::query("COMMIT").execute(&mut *conn).await.map_err(internal)?;

    Ok(Json(SubmitOrderRes{ order_id, ingest_seq }))
}

fn bad<E: std::fmt::Display>((code, e): (u16, E)) -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::from_u16(code).unwrap(), e.to_string())
}
fn internal<E: std::fmt::Display>(e: E) -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}


