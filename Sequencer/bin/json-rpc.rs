use axum_extra::jsonrpc::{JsonRpcMethod, JsonRpcRequest, JsonRpcResponse, RpcRouter};

async fn rpc_book_getTopOfBook(State(state): State<AppState>, req: JsonRpcRequest) -> Json<JsonRpcResponse> {
    #[derive(Deserialize)] struct Params{ pair_id: u32 }
    let p: Params = req.params().unwrap_or_default();
    let ob = TopOfBook{ best_bid: None, best_ask: None }; // fill it
    Json(req.respond_result(serde_json::to_value(ob).unwrap()))
}

pub fn rpc_router(state: AppState) -> Router {
    let router = RpcRouter::new()
        .method("book_getTopOfBook", JsonRpcMethod::new(rpc_book_getTopOfBook))
        ;
    Router::new().merge(router.route("/rpc", post(RpcRouter::into_service))).with_state(state)
}
