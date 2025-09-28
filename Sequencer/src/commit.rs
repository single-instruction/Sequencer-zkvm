use engine::types::{FillDraft, MarketParams, Order};
use tracing::debug;

pub trait PoseidonHasher {
    fn h_bytes(&self, domain_tag: u64, bytes: &[u8]) -> [u8; 32];
    fn h2(&self, domain_tag: u64, a: [u8; 32], b: [u8; 32]) -> [u8; 32];
}

pub struct BlakePoseidonStub;
impl PoseidonHasher for BlakePoseidonStub {
    fn h_bytes(&self, tag: u64, bytes: &[u8]) -> [u8; 32] {
        let mut v = tag.to_le_bytes().to_vec();
        v.extend_from_slice(bytes);
        *blake3::hash(&v).as_bytes()
    }
    fn h2(&self, tag: u64, a: [u8; 32], b: [u8; 32]) -> [u8; 32] {
        let mut v = tag.to_le_bytes().to_vec();
        v.extend_from_slice(&a);
        v.extend_from_slice(&b);
        *blake3::hash(&v).as_bytes()
    }
}

pub mod domains {
    // Original verbose domain tags (placeholders); replace with real ones later.
    pub const ORDER_LEAF: u64 = 0x76C6; // "order_leaf"
    pub const ORDERS_ACC: u64 = 0x72646; // "orders_acc"
    pub const FILL_LEAF: u64 = 0x66C66; // "fill_leaf"
    pub const FILLS_ACC: u64 = 0x66663; // "fills_acc"
    pub const MARKET_LEAF: u64 = 0x6D61726; // "market_leaf"
    pub const MARKETS_ACC: u64 = 0x6D61723; // "markets_acc"
}

pub fn commit_orders<H: PoseidonHasher>(h: &H, orders: &[Order]) -> [u8; 32] {
    use crate::encode::encode_order;
    let mut acc = [0u8; 32];
    for (i, o) in orders.iter().enumerate() {
        let leaf = h.h_bytes(domains::ORDER_LEAF, &encode_order(o));
        acc = h.h2(domains::ORDERS_ACC, acc, leaf);
        if i % 1024 == 0 {
            debug!(i, total = orders.len(), "commit_orders_progress");
        }
    }
    debug!(count = orders.len(), "commit_orders_done");
    acc
}

pub fn commit_fills<H: PoseidonHasher>(h: &H, fills: &[FillDraft]) -> [u8; 32] {
    use crate::encode::encode_fill;
    let mut acc = [0u8; 32];
    for (i, f) in fills.iter().enumerate() {
        let leaf = h.h_bytes(domains::FILL_LEAF, &encode_fill(f));
        acc = h.h2(domains::FILLS_ACC, acc, leaf);
        if i % 1024 == 0 {
            debug!(i, total = fills.len(), "commit_fills_progress");
        }
    }
    debug!(count = fills.len(), "commit_fills_done");
    acc
}

pub fn commit_markets<H: PoseidonHasher>(h: &H, mkts: &[MarketParams]) -> [u8; 32] {
    use crate::encode::encode_market;
    let mut acc = [0u8; 32];
    for (i, m) in mkts.iter().enumerate() {
        let leaf = h.h_bytes(domains::MARKET_LEAF, &encode_market(m));
        acc = h.h2(domains::MARKETS_ACC, acc, leaf);
        if i % 256 == 0 {
            debug!(i, total = mkts.len(), "commit_markets_progress");
        }
    }
    debug!(count = mkts.len(), "commit_markets_done");
    acc
}
