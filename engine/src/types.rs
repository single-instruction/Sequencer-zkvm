#![allow(dead_code)]
use std::cmp::Ordering;

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct PairId(pub u32);

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Side { Bid, Ask }  // Bid=buy, Ask=sell

#[derive(Clone, Debug)]
pub struct MarketParams {
    pub pair_id: PairId,
    pub price_tick: u64,     // min price increment
    pub size_step: u64,      // min size increment
    pub notional_min: u128,  // price * size bounds
    pub notional_max: u128,
    pub maker_bps: u16,
    pub taker_bps: u16,
    pub status: MarketStatus,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MarketStatus { Active, Paused, CancelOnly, Delisted }

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct OrderId(pub u64);

#[derive(Clone, Debug)]
pub struct Order {
    pub order_id: OrderId,
    pub order_hash: [u8; 32],
    pub pair_id: PairId,
    pub side: Side,
    pub price_tick: u64,
    pub amount: u64,
    pub remaining: u64,
    pub time_bucket: u32,
    pub nonce: u64,
    pub ingest_seq: u64, // strict FIFO tiebreaker within price
}

impl Order {
    #[inline] pub fn is_open(&self) -> bool { self.remaining > 0 }
}

impl PartialEq for Order {
    fn eq(&self, other: &Self) -> bool { self.order_id == other.order_id }
}
impl Eq for Order {}

/// Hidden owner mapping (from DB)
pub type PkHash = [u8; 32];

#[derive(Clone, Debug)]
pub struct FillDraft {
    pub batch_id: u64,
    pub match_id: u64,           // unique within batch
    pub pair_id: PairId,
    pub price_tick: u64,
    pub fill_qty: u64,
    pub time_bucket: u32,

    pub buyer_order_id: OrderId,
    pub seller_order_id: OrderId,

    pub buyer_order_hash: [u8; 32],
    pub seller_order_hash: [u8; 32],

    pub buyer_pid: [u8; 32],
    pub seller_pid: [u8; 32],

    pub fee_bps: u16,
    pub fill_salt: Option<[u8; 32]>,
}

#[derive(Copy, Clone, Debug)]
pub struct OrderResidual {
    pub order_id: OrderId,
    pub remaining_before: u64,
    pub remaining_after: u64,
    pub now_filled: bool,
}

#[derive(Copy, Clone, Debug)]
pub struct OrderKey {
    pub side: Side,
    pub price_tick: u64,
    pub ingest_seq: u64,
}

impl OrderKey {
    pub fn cmp(a: &Self, b: &Self) -> Ordering {
        match (a.side, b.side) {
            (Side::Bid, Side::Bid) =>
                a.price_tick.cmp(&b.price_tick).reverse()
                    .then_with(|| a.ingest_seq.cmp(&b.ingest_seq)),
            (Side::Ask, Side::Ask) =>
                a.price_tick.cmp(&b.price_tick)
                    .then_with(|| a.ingest_seq.cmp(&b.ingest_seq)),
            _ => Ordering::Equal,
        }
    }
}



