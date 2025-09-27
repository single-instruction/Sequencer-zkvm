use axum::extract::{WebSocketUpgrade, State};
use axum::response::IntoResponse;
use axum::extract::ws::{Message, WebSocket};
use futures::{StreamExt, SinkExt};

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(mut socket: WebSocket, state: AppState) {
    let mut sub_blocks = state.tx_blocks.subscribe();
    let mut sub_fills  = state.tx_fills.subscribe();
    // simple fanout: send both streams to client
    loop {
        tokio::select! {
            Ok(h) = sub_blocks.recv() => {
                let _ = socket.send(Message::Text(serde_json::to_string(&h).unwrap())).await;
            }
            Ok(f) = sub_fills.recv() => {
                let _ = socket.send(Message::Text(serde_json::to_string(&f).unwrap())).await;
            }
            Some(Ok(msg)) = socket.next() => {
                if let Message::Close(_) = msg { break; }
            }
            else => { break; }
        }
    }
}
