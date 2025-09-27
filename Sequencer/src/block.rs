use crate::types::*;
use crate::commit::{PoseidonHasher, commit_orders, commit_fills, commit_markets};

#[derive(Clone, Copy, Debug)]
pub struct BlockNumber(pub u64);
#[derive(Clone, Copy, Debug)]
pub struct BatchId(pub u64);

#[derive(Clone, Debug)]
pub struct BlockHeader {
    pub block_number: BlockNumber,
    pub batch_id: BatchId,
    pub parent_state_root: [u8;32],
    pub new_state_root: [u8;32],     // set after zk proof
    pub markets_root: [u8;32],
    pub orders_commitment: [u8;32],
    pub fills_commitment: [u8;32],
    pub timestamp_ms: u64,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub header: BlockHeader,
    pub markets_used: Vec<MarketParams>,
    pub orders_snapshot: Vec<Order>,
    pub fills: Vec<FillDraft>,
}

#[async_trait::async_trait]
pub trait Db: Send + Sync + 'static {
    type Tx<'a>: DbTx + Send where Self: 'a;
    async fn begin_repeatable_read(&self) -> anyhow::Result<Self::Tx<'_>>;
}

#[async_trait::async_trait]
pub trait DbTx: Send {
    async fn load_active_markets(&mut self) -> anyhow::Result<Vec<MarketParams>>;
    async fn load_open_orders_snapshot(&mut self) -> anyhow::Result<Vec<Order>>;
    async fn load_owner_pkhash_map_for_orders(
        &mut self, orders: &[Order]
    ) -> anyhow::Result<std::collections::HashMap<u64, PkHash>>;

    async fn insert_fills(&mut self, fills: &[FillDraft]) -> anyhow::Result<()>;
    async fn apply_residuals(&mut self, residuals: &[OrderResidual]) -> anyhow::Result<()>;

    async fn insert_batch_row(&mut self, header: &BlockHeader) -> anyhow::Result<()>;
    async fn link_fills_to_batch(&mut self, block_num: BlockNumber, fills: &[FillDraft]) -> anyhow::Result<()>;

    async fn commit(self) -> anyhow::Result<()>;
}

pub struct BlockBuilder<D: Db, H: PoseidonHasher> {
    db: D,
    hasher: H,
}

impl<D: Db, H: PoseidonHasher> BlockBuilder<D, H> {
    pub fn new(db: D, hasher: H) -> Self { Self { db, hasher } }

    pub async fn build_block(
        &self,
        block_number: BlockNumber,
        batch_id: BatchId,
        parent_state_root: [u8;32],
        timestamp_ms: u64,
        use_fill_salt: bool,
        mut salt_fn: impl FnMut(u64,u64)->[u8;32] + Send,
    ) -> anyhow::Result<Block> {
        let mut tx = self.db.begin_repeatable_read().await?;

        let markets = tx.load_active_markets().await?;
        let markets_root = commit_markets(&self.hasher, &markets);

        let orders = tx.load_open_orders_snapshot().await?;
        let owner_map = tx.load_owner_pkhash_map_for_orders(&orders).await?;

        // group by market
        use std::collections::BTreeMap;
        let mut map: BTreeMap<PairId, (MarketParams, Vec<Order>)> = BTreeMap::new();
        for m in &markets { map.insert(m.pair_id, (m.clone(), Vec::new())); }
        for o in orders.iter().cloned() {
            if let Some((_, v)) = map.get_mut(&o.pair_id) { v.push(o); }
        }

        let mut all_fills = Vec::<FillDraft>::new();
        let mut all_residuals = Vec::<OrderResidual>::new();

        for (pair_id, (mkt, ords)) in map {
            let plan = crate::r#match::match_market(
                pair_id, batch_id.0, &mkt, ords, &owner_map, &self.hasher, use_fill_salt,
                |b, i| salt_fn(b, i),
            );
            all_fills.extend(plan.fills);
            all_residuals.extend(plan.residuals);
        }

        // commitments
        let orders_commitment = commit_orders(&self.hasher, &orders);
        let fills_commitment  = commit_fills(&self.hasher, &all_fills);

        // persist
        tx.insert_fills(&all_fills).await?;
        tx.apply_residuals(&all_residuals).await?;

        let header = BlockHeader {
            block_number, batch_id, parent_state_root,
            new_state_root: [0u8;32], // fill after zk proof
            markets_root, orders_commitment, fills_commitment,
            timestamp_ms,
        };
        tx.insert_batch_row(&header).await?;
        tx.link_fills_to_batch(block_number, &all_fills).await?;
        tx.commit().await?;

        Ok(Block {
            header,
            markets_used: markets,
            orders_snapshot: orders,
            fills: all_fills,
        })
    }
}
