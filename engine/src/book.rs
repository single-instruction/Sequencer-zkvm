use crate::{Order, OrderKey, Side};
use std::collections::BinaryHeap;
use std::cmp::Ordering;

/// Min-heap for asks,
///  max-heap for bids using OrderKey rules.
// #[derive(Default)]
pub struct SideBook {
    side: Side,
    heap: BinaryHeap<BookItem>,
}

#[derive(Clone)]
struct BookItem {
    key: OrderKey,
    idx: usize, // index into orders vec
}

impl PartialEq for BookItem {
    fn eq(&self, other: &Self) -> bool {
        self.key.price_tick == other.key.price_tick &&
        self.key.ingest_seq == other.key.ingest_seq &&
        self.key.side == other.key.side
    }
}
impl Eq for BookItem {}

impl PartialOrd for BookItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for BookItem {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is max-heap; invert for asks
        match self.key.side {
            Side::Bid => OrderKey::cmp(&self.key, &other.key),
            Side::Ask => OrderKey::cmp(&self.key, &other.key).reverse(),
        }
    }
}

pub struct OrderBook {
    pub bids: SideBook,
    pub asks: SideBook,
    pub orders: Vec<Order>, // indexed by BookItem.idx
}

impl OrderBook {
    pub fn from_orders(mut orders: Vec<Order>) -> Self {
        let mut bids = SideBook { side: Side::Bid, heap: BinaryHeap::new() };
        let mut asks = SideBook { side: Side::Ask, heap: BinaryHeap::new() };

        orders.retain(|o| o.remaining > 0);

        for (idx, o) in orders.iter().enumerate() {
            let key = OrderKey { side: o.side, price_tick: o.price_tick, ingest_seq: o.ingest_seq };
            let item = BookItem { key, idx };
            match o.side {
                Side::Bid => bids.heap.push(item),
                Side::Ask => asks.heap.push(item),
            }
        }
        Self { bids, asks, orders }
    }

    pub fn best_bid_idx(&self) -> Option<usize> {
        self.bids.heap.peek().map(|it| it.idx)
    }
    pub fn best_ask_idx(&self) -> Option<usize> {
        self.asks.heap.peek().map(|it| it.idx)
    }

    /// Pop & reinsert if order still has remaining; else drop it.
    pub fn consume_bid_top(&mut self) {
        if let Some(mut it) = self.bids.heap.pop() {
            if self.orders[it.idx].remaining > 0 {
                // reinsert to maintain heap (ingest_seq/price unchanged)
                self.bids.heap.push(it);
            }
        }
    }
    pub fn consume_ask_top(&mut self) {
        if let Some(mut it) = self.asks.heap.pop() {
            if self.orders[it.idx].remaining > 0 {
                self.asks.heap.push(it);
            }
        }
    }

    /// After mutating an order's remaining, call this to drop it if empty and advance the heap.
    pub fn on_fill(&mut self, side: Side) {
        match side {
            Side::Bid => {
                // If top is now empty, pop it
                while let Some(idx) = self.best_bid_idx() {
                    if self.orders[idx].remaining == 0 {
                        self.bids.heap.pop();
                    } else { break; }
                }
            }
            Side::Ask => {
                while let Some(idx) = self.best_ask_idx() {
                    if self.orders[idx].remaining == 0 {
                        self.asks.heap.pop();
                    } else { break; }
                }
            }
        }
    }
}
