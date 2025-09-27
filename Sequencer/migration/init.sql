-- markets
CREATE TABLE IF NOT EXISTS markets (
  pair_id       BIGINT PRIMARY KEY,
  symbol        TEXT NOT NULL,
  price_tick    BIGINT NOT NULL,
  size_step     BIGINT NOT NULL,
  notional_min  NUMERIC(38,0) NOT NULL,
  notional_max  NUMERIC(38,0) NOT NULL,
  maker_bps     INT NOT NULL,
  taker_bps     INT NOT NULL,
  status        SMALLINT NOT NULL,
  params_hash   BYTEA NOT NULL
);

-- orders (public; no identity)
CREATE TABLE IF NOT EXISTS orders (
  order_id      BIGINT PRIMARY KEY,
  order_hash    BYTEA  NOT NULL,
  pair_id       BIGINT NOT NULL REFERENCES markets(pair_id),
  side          SMALLINT NOT NULL,      -- 0=Bid, 1=Ask
  price_tick    BIGINT NOT NULL,
  amount        BIGINT NOT NULL,
  remaining     BIGINT NOT NULL,
  time_bucket   INT    NOT NULL,
  nonce         BIGINT NOT NULL,
  ingest_seq    BIGINT NOT NULL,
  status        SMALLINT NOT NULL DEFAULT 0, -- 0 open, 1 filled, 2 canceled
  created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_orders_pair_price_side ON orders(pair_id, side, price_tick);
CREATE INDEX IF NOT EXISTS idx_orders_open_book ON orders(pair_id, side) WHERE remaining > 0;

-- private owner map (isolate/encrypt this in prod)
CREATE TABLE IF NOT EXISTS order_owners_private (
  order_id BIGINT PRIMARY KEY REFERENCES orders(order_id),
  pk_hash  BYTEA NOT NULL
);

-- fills (public PIDs, not addresses)
CREATE TABLE IF NOT EXISTS fills (
  batch_id          BIGINT NOT NULL,
  match_id          BIGINT NOT NULL,
  pair_id           BIGINT NOT NULL REFERENCES markets(pair_id),
  price_tick        BIGINT NOT NULL,
  fill_qty          BIGINT NOT NULL,
  time_bucket       INT    NOT NULL,
  buyer_order_id    BIGINT NOT NULL REFERENCES orders(order_id),
  seller_order_id   BIGINT NOT NULL REFERENCES orders(order_id),
  buyer_order_hash  BYTEA  NOT NULL,
  seller_order_hash BYTEA  NOT NULL,
  buyer_pid         BYTEA  NOT NULL,
  seller_pid        BYTEA  NOT NULL,
  fee_bps           INT    NOT NULL,
  fill_salt         BYTEA,
  PRIMARY KEY (batch_id, match_id)
);

CREATE INDEX IF NOT EXISTS idx_fills_pair ON fills(pair_id);

-- batches/blocks
CREATE TABLE IF NOT EXISTS batches (
  block_number        BIGINT PRIMARY KEY,
  batch_id            BIGINT UNIQUE NOT NULL,
  parent_state_root   BYTEA  NOT NULL,
  new_state_root      BYTEA  NOT NULL,
  markets_root        BYTEA  NOT NULL,
  orders_commitment   BYTEA  NOT NULL,
  fills_commitment    BYTEA  NOT NULL,
  timestamp_ms        BIGINT NOT NULL,
  proof_tx_hash       BYTEA,
  proof_artifact_uri  TEXT
);

-- optional convenience linking
CREATE TABLE IF NOT EXISTS batch_fills (
  block_number  BIGINT NOT NULL REFERENCES batches(block_number),
  match_id      BIGINT NOT NULL,
  PRIMARY KEY (block_number, match_id)
);

