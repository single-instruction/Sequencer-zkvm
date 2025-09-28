use axum::{http::StatusCode, response::IntoResponse};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubmitOrderReq {
    pair_id: u32,
    side: u8,            // 0 = buy, 1 = sell
    price_tick: u64,
    amount: u64,
    time_bucket: u32,
    nonce: u64,
    order_hash: String,  // hex
    pk_hash: String,     // hex
}

#[derive(Serialize)]
struct SubmitOrderRes { order_id: u64, ingest_seq: u64 }

// simple input guard; expand as needed
fn valid_order(req: &SubmitOrderReq) -> bool {
    (req.side == 0 || req.side == 1) && req.amount > 0 && req.price_tick > 0
}

async fn post_order(Json(req): Json<SubmitOrderReq>) -> impl IntoResponse {
    if !valid_order(&req) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error":"invalid order"})));
    }
    let res = SubmitOrderRes { order_id: 42, ingest_seq: 7 };

    (
        StatusCode::CREATED,
        Json(res),
    )
}
