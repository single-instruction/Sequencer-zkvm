use axum::{
    routing::{get, post},
    extract::{Path, State, Query},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::RwLock;
use tracing::info;


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
    pub blocks: HashMap<u64, BlockHeaderDTO>,
}

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<RwLock<MockStore>>,
}

fn seed_mock() -> MockStore {
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

    MockStore { markets, orderbooks, fills, blocks }
}

#[derive(Deserialize)]
struct FillsQuery { pair_id: Option<u32>, batch_id: Option<u64>, limit: Option<usize> }

async fn get_markets(State(state): State<AppState>) -> Json<Vec<MarketDTO>> {
    Json(state.store.read().await.markets.clone())
}

async fn get_orderbook(State(state): State<AppState>, Path(pair): Path<u32>) -> Json<TopOfBook> {
    let ob = state.store.read().await.orderbooks.get(&pair).cloned()
        .unwrap_or(TopOfBook { best_bid: None, best_ask: None });
    Json(ob)
}

async fn get_fills(State(state): State<AppState>, Query(q): Query<FillsQuery>) -> Json<Vec<FillDTO>> {
    let s = state.store.read().await;
    let mut v = s.fills.clone();
    if let Some(p) = q.pair_id { v.retain(|f| f.pair_id == p); }
    if let Some(b) = q.batch_id { v.retain(|f| f.batch_id == b); }
    let n = q.limit.unwrap_or(200).min(1000);
    v.truncate(n);
    Json(v)
}

async fn get_block(State(state): State<AppState>, Path(n): Path<u64>) -> axum::response::Result<Json<BlockHeaderDTO>> {
    match state.store.read().await.blocks.get(&n).cloned() {
        Some(block) => Ok(Json(block)),
        None => Err(axum::http::StatusCode::NOT_FOUND.into()),
    }
}

#[derive(Deserialize)]
struct SubmitOrderReq {
    pair_id: u32, side: u8, price_tick: u64, amount: u64,
    time_bucket: u32, nonce: u64, order_hash: String, pk_hash: String
}
#[derive(Serialize)]
struct SubmitOrderRes { order_id: u64, ingest_seq: u64 }

async fn post_order(Json(_req): Json<SubmitOrderReq>) -> Json<SubmitOrderRes> {
    // Mock: just return an ID/seq. Wire to DB later.
    Json(SubmitOrderRes { order_id: 42, ingest_seq: 7 })
}

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

// ---------- JSON-RPC ----------

// ---------- Main ----------
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let state = AppState { store: Arc::new(RwLock::new(seed_mock())) };
    let app = Router::new()
        // REST
        .route("/v1/markets", get(get_markets))
        .route("/v1/orderbook/:pair_id", get(get_orderbook))
        .route("/v1/fills", get(get_fills))
        .route("/v1/blocks/:block_number", get(get_block))
        .route("/v1/orders", post(post_order))
        .route("/rpc", post(rpc_handler))
        .with_state(state);

    let addr: SocketAddr = "0.0.0.0:8080".parse().unwrap();
    info!("mock sequencer listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
