use crate::types::{Order, FillDraft, MarketParams};

pub trait PoseidonHasher {
    fn h_bytes(&self, domain_tag: u64, bytes: &[u8]) -> [u8;32];
    fn h2(&self, domain_tag: u64, a: [u8;32], b: [u8;32]) -> [u8;32];
}

pub struct BlakePoseidonStub;
impl PoseidonHasher for BlakePoseidonStub {
    fn h_bytes(&self, tag: u64, bytes: &[u8]) -> [u8;32] {
        let mut v = tag.to_le_bytes().to_vec();
        v.extend_from_slice(bytes);
        *blake3::hash(&v).as_bytes()
    }
    fn h2(&self, tag: u64, a: [u8;32], b: [u8;32]) -> [u8;32] {
        let mut v = tag.to_le_bytes().to_vec();
        v.extend_from_slice(&a);
        v.extend_from_slice(&b);
        *blake3::hash(&v).as_bytes()
    }
}

pub mod domains {
    pub const ORDER_LEAF: u64 = 0x6F726465725F6C656166;   // "order_leaf"
    pub const ORDERS_ACC: u64 = 0x6F72646572735F616363;   // "orders_acc"
    pub const FILL_LEAF:  u64 = 0x66696C6C5F6C656166;     // "fill_leaf"
    pub const FILLS_ACC:  u64 = 0x66696C6C735F616363;     // "fills_acc"
    pub const MARKET_LEAF:u64 = 0x6D61726B65745F6C656166; // "market_leaf"
    pub const MARKETS_ACC:u64 = 0x6D61726B6574735F616363; // "markets_acc"
}

pub fn commit_orders<H: PoseidonHasher>(h: &H, orders: &[Order]) -> [u8;32] {
    use crate::encode::encode_order;
    let mut acc = [0u8;32];
    for o in orders {
        let leaf = h.h_bytes(domains::ORDER_LEAF, &encode_order(o));
        acc = h.h2(domains::ORDERS_ACC, acc, leaf);
    }
    acc
}

pub fn commit_fills<H: PoseidonHasher>(h: &H, fills: &[FillDraft]) -> [u8;32] {
    use crate::encode::encode_fill;
    let mut acc = [0u8;32];
    for f in fills {
        let leaf = h.h_bytes(domains::FILL_LEAF, &encode_fill(f));
        acc = h.h2(domains::FILLS_ACC, acc, leaf);
    }
    acc
}

pub fn commit_markets<H: PoseidonHasher>(h: &H, mkts: &[MarketParams]) -> [u8;32] {
    use crate::encode::encode_market;
    let mut acc = [0u8;32];
    for m in mkts {
        let leaf = h.h_bytes(domains::MARKET_LEAF, &encode_market(m));
        acc = h.h2(domains::MARKETS_ACC, acc, leaf);
    }
    acc
}

