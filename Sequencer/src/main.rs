use axum::{
    routing::{get, post},
    extract::{Path, State, Query},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::RwLock;
use tracing::{info, debug, warn};
use tower_http::trace::{TraceLayer, DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse};
use tower_http::request_id::{PropagateRequestIdLayer, SetRequestIdLayer};
use http::header::HeaderName;
use tower_http::request_id::MakeRequestUuid;
use rand::{Rng, seq::SliceRandom};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{interval, Duration};



#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PairId(pub u32);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketDTO {
    pub pair_id: u32,
    pub symbol: String,
    pub price_tick: u64,
    pub size_step: u64,
    pub maker_bps: u16,
    pub taker_bps: u16,
    pub status: u8, // 0 Active, 1 Paused, 2 CancelOnly, 3 Delisted
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TopOfBook {
    pub best_bid: Option<(u64 /*price*/, u64 /*qty*/ )>,
    pub best_ask: Option<(u64, u64)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FillDTO {
    pub batch_id: u64,
    pub match_id: u64,
    pub pair_id: u32,
    pub price_tick: u64,
    pub fill_qty: u64,
    pub time_bucket: u32,
    pub buyer_pid: String,   // hex
    pub seller_pid: String,  // hex
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderDTO {
    pub order_id: u64,
    pub pair_id: u32,
    pub side: u8,          // 0 bid, 1 ask
    pub price_tick: u64,
    pub amount: u64,
    pub remaining: u64,
    pub time_bucket: u32,
    pub ingest_seq: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockHeaderDTO {
    pub block_number: u64,
    pub batch_id: u64,
    pub parent_state_root: String,   // hex
    pub new_state_root: String,      // hex
    pub markets_root: String,        // hex
    pub orders_commitment: String,   // hex
    pub fills_commitment: String,    // hex
    pub timestamp_ms: u64,
}

#[derive(Deserialize)]
struct JsonRpcReq {
    jsonrpc: String,
    method: String,
    #[serde(default)]
    params: Value,      // object or array
    id: Value,          // number or string
}

#[derive(Serialize)]
struct JsonRpcRes {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
    id: Value,
}

#[derive(Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}
#[derive(Default)]
pub struct MockStore {
    pub markets: Vec<MarketDTO>,
    pub orderbooks: HashMap<u32, TopOfBook>,
    pub fills: Vec<FillDTO>,
    pub orders: Vec<OrderDTO>,
    pub blocks: HashMap<u64, BlockHeaderDTO>,
}

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<RwLock<MockStore>>,
}

fn seed_mock() -> MockStore {
    debug!(markets = 2, orderbooks = 2, fills = 2, blocks = 1, "seeding_mock_data");
    let markets = vec![
        MarketDTO { pair_id: 1, symbol: "POL-ETH".into(), price_tick: 1, size_step: 1, maker_bps: 0, taker_bps: 5, status: 0 },
        MarketDTO { pair_id: 2, symbol: "USDC-ETH".into(), price_tick: 1, size_step: 1, maker_bps: 0, taker_bps: 5, status: 0 },
    ];
    let mut orderbooks = HashMap::new();
    orderbooks.insert(1, TopOfBook { best_bid: Some( (100, 37) ), best_ask: Some( (101, 42) ) });
    orderbooks.insert(2, TopOfBook { best_bid: Some( (2000, 1000) ), best_ask: Some( (2001, 900) ) });

    let fills = vec![
        FillDTO {
            batch_id: 1, match_id: 1, pair_id: 1, price_tick: 101, fill_qty: 7, time_bucket: 0,
            buyer_pid: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            seller_pid:"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
        },
        FillDTO {
            batch_id: 1, match_id: 2, pair_id: 1, price_tick: 101, fill_qty: 3, time_bucket: 0,
            buyer_pid: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            seller_pid:"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".into(),
        }
    ];

    let mut blocks = HashMap::new();
    blocks.insert(1, BlockHeaderDTO {
        block_number: 1,
        batch_id: 1,
        parent_state_root: "00".repeat(32),
        new_state_root:     "11".repeat(32),
        markets_root:       "22".repeat(32),
        orders_commitment:  "33".repeat(32),
        fills_commitment:   "44".repeat(32),
        timestamp_ms: 1_700_000_000_000,
    });

    // seed some mock orders (between 12 and 20)
    let mut orders = Vec::new();
    let mut ingest_seq: u64 = 1;
    for pair in [1u32,2u32] {
        for _ in 0..6 { // 6 per market
            orders.push(OrderDTO {
                order_id: ingest_seq,
                pair_id: pair,
                side: if ingest_seq % 2 == 0 { 0 } else { 1 },
                price_tick: if pair == 1 { 100 + (ingest_seq % 5) } else { 2000 + (ingest_seq % 5) } ,
                amount: 50 + (ingest_seq % 25),
                remaining: 50 + (ingest_seq % 25),
                time_bucket: 0,
                ingest_seq,
            });
            ingest_seq += 1;
        }
    }

    MockStore { markets, orderbooks, fills, orders, blocks }
}

#[derive(Deserialize, Debug)]
struct FillsQuery { pair_id: Option<u32>, batch_id: Option<u64>, limit: Option<usize> }

#[tracing::instrument(level="info", skip(state))]
async fn get_markets(State(state): State<AppState>) -> Json<Vec<MarketDTO>> {
    let out = state.store.read().await.markets.clone();
    debug!(count = out.len(), "get_markets_return");
    Json(out)
}

#[tracing::instrument(level="info", skip(state))]
async fn get_orderbook(State(state): State<AppState>, Path(pair): Path<u32>) -> Json<TopOfBook> {
    let ob = state.store.read().await.orderbooks.get(&pair).cloned()
        .unwrap_or(TopOfBook { best_bid: None, best_ask: None });
    debug!(pair, has_bid = ob.best_bid.is_some(), has_ask = ob.best_ask.is_some(), "get_orderbook_return");
    Json(ob)
}

#[tracing::instrument(level="info", skip(state, q))]
async fn get_fills(State(state): State<AppState>, Query(q): Query<FillsQuery>) -> Json<Vec<FillDTO>> {
    let s = state.store.read().await;
    let mut v = s.fills.clone();
    if let Some(p) = q.pair_id { v.retain(|f| f.pair_id == p); }
    if let Some(b) = q.batch_id { v.retain(|f| f.batch_id == b); }
    let n = q.limit.unwrap_or(200).min(1000);
    v.truncate(n);
    debug!(returned = v.len(), "get_fills_return");
    Json(v)
}

#[tracing::instrument(level="info", skip(state), fields(block_number = n))]
async fn get_block(State(state): State<AppState>, Path(n): Path<u64>) -> axum::response::Result<Json<BlockHeaderDTO>> {
    match state.store.read().await.blocks.get(&n).cloned() {
        Some(block) => {
            debug!("block_found");
            Ok(Json(block))
        },
        None => {
            warn!("block_not_found");
            Err(axum::http::StatusCode::NOT_FOUND.into())
        },
    }
}

#[derive(Deserialize)]
struct SubmitOrderReq {
    pair_id: u32, side: u8, price_tick: u64, amount: u64,
    time_bucket: u32, nonce: u64, order_hash: String, pk_hash: String
}
#[derive(Serialize, Debug)]
struct SubmitOrderRes { order_id: u64, ingest_seq: u64 }

#[tracing::instrument(level="info", skip(_req))]
async fn post_order(Json(_req): Json<SubmitOrderReq>) -> Json<SubmitOrderRes> {
    Json(SubmitOrderRes { order_id: 42, ingest_seq: 7 })
}

#[tracing::instrument(level="info", skip(state, req), fields(method = %req.method))]
async fn rpc_handler(State(state): State<AppState>, Json(req): Json<JsonRpcReq>) -> Json<JsonRpcRes> {
    let mk_ok = |v: Value| JsonRpcRes{ jsonrpc: "2.0", result: Some(v), error: None, id: req.id.clone() };
    let mk_err = |code: i64, msg: &str| JsonRpcRes{
        jsonrpc: "2.0",
        result: None,
        error: Some(JsonRpcError{ code, message: msg.to_string(), data: None }),
        id: req.id.clone(),
    };

    match req.method.as_str() {
        "book_getTopOfBook" => {
            #[derive(Deserialize)] struct P { pair_id: u32 }
            let p: P = serde_json::from_value(req.params.clone()).unwrap_or(P{ pair_id: 1 });
            let ob = state.store.read().await.orderbooks.get(&p.pair_id).cloned()
                .unwrap_or(TopOfBook{ best_bid: None, best_ask: None });
            Json(mk_ok(serde_json::to_value(ob).unwrap()))
        }
        "batch_getHeader" => {
            #[derive(Deserialize)] struct P { block_number: u64 }
            let p: P = serde_json::from_value(req.params.clone()).unwrap_or(P{ block_number: 1 });
            match state.store.read().await.blocks.get(&p.block_number).cloned() {
                Some(h) => Json(mk_ok(serde_json::to_value(h).unwrap())),
                None => Json(mk_err(-32602, "not found")),
            }
        }
        "fills_getSince" => {
            #[derive(Deserialize, Default)] struct P { batch_id: Option<u64>, pair_id: Option<u32>, limit: Option<usize> }
            let p: P = serde_json::from_value(req.params.clone()).unwrap_or_default();
            let mut v = state.store.read().await.fills.clone();
            if let Some(b) = p.batch_id { v.retain(|f| f.batch_id == b); }
            if let Some(pr) = p.pair_id { v.retain(|f| f.pair_id == pr); }
            v.truncate(p.limit.unwrap_or(200).min(1000));
            Json(mk_ok(serde_json::to_value(v).unwrap()))
        }
        _ => Json(mk_err(-32601, "method not found")),
    }
}

async fn start_seeder(state: AppState) {
    let mut ticker = interval(Duration::from_secs(2));
    let mut block_number: u64 = 2;

    loop {
        ticker.tick().await;
    let mut store = state.store.write().await;
    let mut rng = rand::thread_rng();

        for (pair_id, ob) in store.orderbooks.iter_mut() {
            let mid = rng.gen_range(90..110) as u64 * (*pair_id as u64);
            ob.best_bid = Some((mid, rng.gen_range(1..100)));
            ob.best_ask = Some((mid + 1, rng.gen_range(1..100)));
        }

        // target number of orders between 9 and 60
        let target_orders = rng.gen_range(9..=60);
        let mut next_order_id = (store.orders.len() as u64) + 1;
        while store.orders.len() < target_orders {
            let pair_id = match store.markets.choose(&mut rng) { Some(mkt) => mkt.pair_id, None => break };
            let side = if rng.gen_bool(0.5) { 0 } else { 1 };
            let price_base = if pair_id == 1 { 100 } else { 2000 };
            let price_tick = price_base + rng.gen_range(0..10) as u64;
            let amount = rng.gen_range(10..200) as u64;
            store.orders.push(OrderDTO {
                order_id: next_order_id,
                pair_id,
                side,
                price_tick,
                amount,
                remaining: amount,
                time_bucket: rng.gen_range(0..10),
                ingest_seq: next_order_id,
            });
            next_order_id += 1;
        }
        // If we have more than target (from previous cycles), randomly drop extras
        while store.orders.len() > target_orders {
            let idx = rng.gen_range(0..store.orders.len());
            store.orders.swap_remove(idx);
        }

        // generate 7-9 fills per tick
        let fill_count = rng.gen_range(7..=9);
        for _ in 0..fill_count {
            let pair_id = match store.markets.choose(&mut rng) { Some(m) => m.pair_id, None => break };
            let fill = FillDTO {
                batch_id: block_number,
                match_id: store.fills.len() as u64 + 1,
                pair_id,
                price_tick: rng.gen_range(90..110) * pair_id as u64,
                fill_qty: rng.gen_range(1..50),
                time_bucket: rng.gen_range(0..10),
                buyer_pid: format!("{:064x}", rng.gen::<u128>()),
                seller_pid: format!("{:064x}", rng.gen::<u128>()),
            };
            store.fills.push(fill);
        }

        // --- Add a new block header ---
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        store.blocks.insert(block_number, BlockHeaderDTO {
            block_number,
            batch_id: block_number,
            parent_state_root: format!("{:064x}", rng.gen::<u128>()),
            new_state_root: format!("{:064x}", rng.gen::<u128>()),
            markets_root: format!("{:064x}", rng.gen::<u128>()),
            orders_commitment: format!("{:064x}", rng.gen::<u128>()),
            fills_commitment: format!("{:064x}", rng.gen::<u128>()),
            timestamp_ms: ts,
        });
        block_number += 1;
    }
}


// ---------- Main ----------
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,sequencer=debug,engine=info".into())
        )
        .with_target(true)
        .compact()
        .init();

    debug!(markets = 2, orderbooks = 2, fills = 2, blocks = 1, "seeding_mock_data");

    let state = AppState { store: Arc::new(RwLock::new(seed_mock())) };

    tokio::spawn(start_seeder(state.clone()));

    let request_id_header = HeaderName::from_static("x-request-id");

    let app = Router::new()
        // REST
        .route("/v1/markets", get(get_markets))
        .route("/v1/orderbook/:pair_id", get(get_orderbook))
        .route("/v1/fills", get(get_fills))
        .route("/v1/blocks/:block_number", get(get_block))
        .route("/v1/orders", post(post_order))
        .route("/rpc", post(rpc_handler))
        .with_state(state)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(tracing::Level::INFO))
                .on_request(DefaultOnRequest::new().level(tracing::Level::INFO))
                .on_response(DefaultOnResponse::new().level(tracing::Level::INFO))
        )
        .layer(PropagateRequestIdLayer::new(request_id_header.clone()))
        .layer(SetRequestIdLayer::new(request_id_header.clone(), MakeRequestUuid));

    let addr: SocketAddr = "0.0.0.0:8080".parse().unwrap();
    info!(%addr, "mock sequencer listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
