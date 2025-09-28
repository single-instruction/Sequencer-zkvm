use axum::{routing::{get, post}, extract::{Path, State}, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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

pub async fn rpc_handler(State(state): State<crate::main::AppState>, Json(req): Json<JsonRpcReq>) -> Json<JsonRpcRes> {
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
                .unwrap_or(crate::main::TopOfBook{ best_bid: None, best_ask: None });
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
