use crate::block::{Db, DbTx, BlockHeader, BlockNumber};
use crate::types::*;
use anyhow::Result;
use sqlx::{Pool, Postgres, Acquire};

pub struct PgDb {
    pool: Pool<Postgres>,
}
impl PgDb {
    pub fn new(pool: Pool<Postgres>) -> Self { Self { pool } }
}

#[async_trait::async_trait]
impl Db for PgDb {
    type Tx<'a> = PgTx<'a>;
    async fn begin_repeatable_read(&self) -> Result<Self::Tx<'_>> {
        let mut conn = self.pool.acquire().await?;
        sqlx::query("BEGIN ISOLATION LEVEL REPEATABLE READ").execute(&mut *conn).await?;
        Ok(PgTx { conn })
    }
}

pub struct PgTx<'a> {
    conn: sqlx::pool::PoolConnection<Postgres>,
}

#[async_trait::async_trait]
impl<'a> DbTx for PgTx<'a> {
    async fn load_active_markets(&mut self) -> Result<Vec<MarketParams>> {
        let rows = sqlx::query!(
            r#"SELECT pair_id, symbol, price_tick, size_step, notional_min, notional_max,
                      maker_bps, taker_bps, status
               FROM markets WHERE status IN (0,1,2)"#
        ).fetch_all(&mut self.conn).await?;

        let mut out = Vec::new();
        for r in rows {
            out.push(MarketParams{
                pair_id: PairId(r.pair_id as u32),
                price_tick: r.price_tick as u64,
                size_step: r.size_step as u64,
                notional_min: r.notional_min.parse::<u128>().unwrap_or(0),
                notional_max: r.notional_max.parse::<u128>().unwrap_or(u128::MAX),
                maker_bps: r.maker_bps as u16,
                taker_bps: r.taker_bps as u16,
                status: match r.status {
                    0 => MarketStatus::Active,
                    1 => MarketStatus::Paused,
                    2 => MarketStatus::CancelOnly,
                    _ => MarketStatus::Delisted
                }
            });
        }
        Ok(out)
    }

    async fn load_open_orders_snapshot(&mut self) -> Result<Vec<Order>> {
        let rows = sqlx::query!(
            r#"SELECT order_id, order_hash, pair_id, side, price_tick, amount, remaining,
                      time_bucket, nonce, ingest_seq
               FROM orders WHERE remaining > 0
               ORDER BY pair_id, side, price_tick, ingest_seq"#
        ).fetch_all(&mut self.conn).await?;

        let mut out = Vec::new();
        for r in rows {
            let mut hash = [0u8;32];
            hash.copy_from_slice(&r.order_hash);
            out.push(Order{
                order_id: OrderId(r.order_id as u64),
                order_hash: hash,
                pair_id: PairId(r.pair_id as u32),
                side: if r.side == 0 { Side::Bid } else { Side::Ask },
                price_tick: r.price_tick as u64,
                amount: r.amount as u64,
                remaining: r.remaining as u64,
                time_bucket: r.time_bucket as u32,
                nonce: r.nonce as u64,
                ingest_seq: r.ingest_seq as u64,
            });
        }
        Ok(out)
    }

    async fn load_owner_pkhash_map_for_orders(
        &mut self, orders: &[Order]
    ) -> Result<std::collections::HashMap<u64, PkHash>> {
        use std::collections::HashMap;
        if orders.is_empty() { return Ok(HashMap::new()); }
        let ids: Vec<i64> = orders.iter().map(|o| o.order_id.0 as i64).collect();
        let rows = sqlx::query!(
            r#"SELECT order_id, pk_hash FROM order_owners_private WHERE order_id = ANY($1)"#, &ids
        ).fetch_all(&mut self.conn).await?;
        let mut map = HashMap::new();
        for r in rows {
            let mut pk = [0u8;32];
            pk.copy_from_slice(&r.pk_hash);
            map.insert(r.order_id as u64, pk);
        }
        Ok(map)
    }

    async fn insert_fills(&mut self, fills: &[FillDraft]) -> Result<()> {
        for f in fills {
            sqlx::query!(
                r#"INSERT INTO fills
                   (batch_id, match_id, pair_id, price_tick, fill_qty, time_bucket,
                    buyer_order_id, seller_order_id, buyer_order_hash, seller_order_hash,
                    buyer_pid, seller_pid, fee_bps, fill_salt)
                  VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)"#,
                f.batch_id as i64,
                f.match_id as i64,
                f.pair_id.0 as i64,
                f.price_tick as i64,
                f.fill_qty as i64,
                f.time_bucket as i32,
                f.buyer_order_id.0 as i64,
                f.seller_order_id.0 as i64,
                &f.buyer_order_hash[..],
                &f.seller_order_hash[..],
                &f.buyer_pid[..],
                &f.seller_pid[..],
                f.fee_bps as i32,
                f.fill_salt.as_ref().map(|s| &s[..])
            ).execute(&mut self.conn).await?;
        }
        Ok(())
    }

    async fn apply_residuals(&mut self, residuals: &[OrderResidual]) -> Result<()> {
        for r in residuals {
            let status = if r.now_filled { 1 } else { 0 };
            sqlx::query!(
                r#"UPDATE orders
                   SET remaining = $1, status = $2, updated_at = now()
                   WHERE order_id = $3"#,
                r.remaining_after as i64, status as i32, r.order_id.0 as i64
            ).execute(&mut self.conn).await?;
        }
        Ok(())
    }

    async fn insert_batch_row(&mut self, h: &BlockHeader) -> Result<()> {
        sqlx::query!(
            r#"INSERT INTO batches
               (block_number, batch_id, parent_state_root, new_state_root,
                markets_root, orders_commitment, fills_commitment, timestamp_ms)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8)"#,
            h.block_number.0 as i64,
            h.batch_id.0 as i64,
            &h.parent_state_root[..],
            &h.new_state_root[..],
            &h.markets_root[..],
            &h.orders_commitment[..],
            &h.fills_commitment[..],
            h.timestamp_ms as i64
        ).execute(&mut self.conn).await?;
        Ok(())
    }

    async fn link_fills_to_batch(&mut self, block_num: BlockNumber, fills: &[FillDraft]) -> Result<()> {
        for f in fills {
            sqlx::query!(
                r#"INSERT INTO batch_fills (block_number, match_id) VALUES ($1,$2)"#,
                block_num.0 as i64, f.match_id as i64
            ).execute(&mut self.conn).await?;
        }
        Ok(())
    }

    async fn commit(mut self) -> Result<()> {
        sqlx::query("COMMIT").execute(&mut self.conn).await?;
        Ok(())
    }
}
