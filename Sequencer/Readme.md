API design
Public REST

POST /v1/orders — intake signed orders (returns order_id, order_hash).
GET /v1/markets — list active markets.
GET /v1/orderbook/:pair_id — top-of-book or full L2 snapshot.
GET /v1/fills?pair_id&batch_id — recent fills / batch fills.
GET /v1/blocks/:block_number — block header + commitments.
GET /healthz — liveness.
GET /metrics —

JSON-RPC

book_getTopOfBook(pair_id)
book_getSnapshot(pair_id,depth)
order_submit(order) (EIP-712 digest + sig already validated client-side or do minimal check server-side)
batch_getHeader(block_number)
fills_getSince(batch_id, pair_id)
state_getRoot(block_number)

WebSocket channels

/ws → topics:

book.{pair_id}
fills.{pair_id}
blocks.new
