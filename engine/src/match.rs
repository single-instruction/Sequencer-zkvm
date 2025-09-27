use crate::{
    types::Order, OrderResidual, FillDraft, PkHash, PairId, Side, MarketParams,OrderKey,
    book::OrderBook,
    pid::{derive_pid, Poseidon32},
};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct ExecutionPlan {
    pub pair_id: PairId,
    pub batch_id: u64,
    pub fills: Vec<FillDraft>,
    pub residuals: Vec<OrderResidual>,
}

pub fn match_market<H: Poseidon32>(
    pair_id: PairId,
    batch_id: u64,
    market: &MarketParams,
    orders: Vec<Order>, // not used after building the book; no `mut` needed
    owner_map: &HashMap<u64, PkHash>,
    hasher: &H,
    use_fill_salt: bool,
    mut fill_salt_fn: impl FnMut(u64,u64) -> [u8;32],
) -> ExecutionPlan {
    assert!(market.pair_id == pair_id, "market/pair mismatch");

    let mut book = OrderBook::from_orders(orders);
    let mut fills = Vec::new();
    let mut residuals: HashMap<u64, OrderResidual> = HashMap::new();
    let mut match_seq: u64 = 0;
    let taker_fee = market.taker_bps;

    loop {
        let (bi, ai) = match (book.best_bid_idx(), book.best_ask_idx()) {
            (Some(b), Some(a)) => (b, a),
            _ => break,
        };
        if book.orders[bi].price_tick < book.orders[ai].price_tick {
            break;
        }

        // ---- snapshot needed fields as VALUES (no long-lived borrows) ----
        let qty        = book.orders[bi].remaining.min(book.orders[ai].remaining);
        let price      = book.orders[ai].price_tick; // maker = resting ask
        let bid_id     = book.orders[bi].order_id;
        let ask_id     = book.orders[ai].order_id;
        let bid_hash   = book.orders[bi].order_hash;
        let ask_hash   = book.orders[ai].order_hash;
        let bid_tb     = book.orders[bi].time_bucket;
        let ask_tb     = book.orders[ai].time_bucket;

        match_seq += 1;
        let match_id = match_seq;

        // ---- take disjoint &mut using split_at_mut ----
        let (lo, hi) = if bi < ai { (bi, ai) } else { (ai, bi) };
        let (left, right) = book.orders.split_at_mut(hi);
        let (bid_mut, ask_mut) = if bi < ai {
            (&mut left[lo], &mut right[0])
        } else {
            (&mut right[0], &mut left[lo])
        };

        let (b_before, a_before) = (bid_mut.remaining, ask_mut.remaining);
        bid_mut.remaining = b_before - qty;
        ask_mut.remaining = a_before - qty;

        // PIDs
        let buyer_pk  = owner_map.get(&bid_id.0).expect("missing buyer pk_hash");
        let seller_pk = owner_map.get(&ask_id.0).expect("missing seller pk_hash");
        let salt      = if use_fill_salt { Some(fill_salt_fn(batch_id, match_id)) } else { None };
        let buyer_pid  = derive_pid(hasher, buyer_pk,  batch_id, match_id, salt);
        let seller_pid = derive_pid(hasher, seller_pk, batch_id, match_id, salt);

        // record fill
        fills.push(FillDraft {
            batch_id,
            match_id,
            pair_id,
            price_tick: price,
            fill_qty: qty,
            time_bucket: bid_tb.max(ask_tb),
            buyer_order_id: bid_id,
            seller_order_id: ask_id,
            buyer_order_hash: bid_hash,
            seller_order_hash: ask_hash,
            buyer_pid,
            seller_pid,
            fee_bps: taker_fee,
            fill_salt: salt,
        });

        // residuals
        residuals.entry(bid_id.0)
            .and_modify(|r| { r.remaining_after = bid_mut.remaining; r.now_filled = r.remaining_after == 0 })
            .or_insert(OrderResidual {
                order_id: bid_id,
                remaining_before: b_before,
                remaining_after: bid_mut.remaining,
                now_filled: bid_mut.remaining == 0,
            });
        residuals.entry(ask_id.0)
            .and_modify(|r| { r.remaining_after = ask_mut.remaining; r.now_filled = r.remaining_after == 0 })
            .or_insert(OrderResidual {
                order_id: ask_id,
                remaining_before: a_before,
                remaining_after: ask_mut.remaining,
                now_filled: ask_mut.remaining == 0,
            });

        book.on_fill(Side::Bid);
        book.on_fill(Side::Ask);
    }

    ExecutionPlan {
        pair_id,
        batch_id,
        fills,
        residuals: residuals.into_values().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{types::*, pid::StubPoseidon};
    use std::collections::HashMap;

    #[test]
    fn simple_cross() {
        let pair = PairId(1);
        let market = MarketParams {
            pair_id: pair,
            price_tick: 1, size_step: 1,
            notional_min: 0, notional_max: u128::MAX,
            maker_bps: 0, taker_bps: 0, status: MarketStatus::Active
        };
        let mut orders = vec![
            Order{ order_id: OrderId(1), order_hash:[1;32], pair_id:pair, side:Side::Bid, price_tick:100, amount:10, remaining:10, time_bucket:0, nonce:1, ingest_seq: 10 },
            Order{ order_id: OrderId(2), order_hash:[2;32], pair_id:pair, side:Side::Ask, price_tick: 95, amount: 7, remaining: 7, time_bucket:0, nonce:2, ingest_seq: 11 },
            Order{ order_id: OrderId(3), order_hash:[3;32], pair_id:pair, side:Side::Ask, price_tick:100, amount: 8, remaining: 8, time_bucket:0, nonce:3, ingest_seq: 12 },
        ];
        // per-market ingest_seq matters only within same side & price—already set.

        let mut owners: HashMap<u64, PkHash> = HashMap::new();
        owners.insert(1, [0xAA;32]);
        owners.insert(2, [0xBB;32]);
        owners.insert(3, [0xCC;32]);

        let plan = match_market(
            pair, 42, &market, orders.clone(), &owners,
            &StubPoseidon, false, |_b,_m| [0u8;32]
        );

        // First fill: bid(100,ingest=10) vs ask(95,ingest=11) at price 95 for qty 7
        assert_eq!(plan.fills[0].price_tick, 95);
        assert_eq!(plan.fills[0].fill_qty, 7);
        assert_eq!(plan.fills[0].buyer_order_id.0, 1);
        assert_eq!(plan.fills[0].seller_order_id.0, 2);

        assert_eq!(plan.fills[1].price_tick, 100);
        assert_eq!(plan.fills[1].fill_qty, 3);
        assert_eq!(plan.fills[1].seller_order_id.0, 3);

        let mut by_id = plan.residuals.iter().map(|r|(r.order_id.0, r)).collect::<std::collections::HashMap<_,_>>();
        assert_eq!(by_id.get(&1).unwrap().remaining_after, 0);
        assert_eq!(by_id.get(&2).unwrap().remaining_after, 0);
        assert_eq!(by_id.get(&3).unwrap().remaining_after, 5);
    }
}

#[cfg(test)]
mod exec_tests {
    use super::*;
    use crate::pid::StubPoseidon;
    use crate::types::*;

    fn mk_order(
        id: u64, side: Side, px: u64, amt: u64, rem: u64, seq: u64, tb: u32
    ) -> Order {
        Order {
            order_id: OrderId(id),
            order_hash: {
                let mut h = [0u8;32];
                h[24..].copy_from_slice(&id.to_be_bytes());
                h
            },
            pair_id: PairId(1),
            side, price_tick: px,
            amount: amt, remaining: rem,
            time_bucket: tb,
            nonce: id,
            ingest_seq: seq,
        }
    }

    fn owners(ids: &[u64]) -> HashMap<u64, PkHash> {
        let mut m = HashMap::new();
        for &id in ids {
            let mut pk = [0u8;32];
            pk[8..16].copy_from_slice(&id.to_be_bytes());
            m.insert(id, pk);
        }
        m
    }

    fn market() -> MarketParams {
        MarketParams{
            pair_id: PairId(1),
            price_tick: 1, size_step: 1,
            notional_min: 0, notional_max: u128::MAX,
            maker_bps: 3, taker_bps: 7,
            status: MarketStatus::Active
        }
    }

    #[test]
    fn fifo_same_price_on_ask_side() {
        // Two asks @100, different ingest_seq; one bid crosses all
        let a1 = mk_order(1, Side::Ask, 100, 3, 3, 10, 0);
        let a2 = mk_order(2, Side::Ask, 100, 5, 5, 11, 0);
        let b  = mk_order(3, Side::Bid, 100, 7, 7, 20, 0);

        let owners = owners(&[1,2,3]);
        let plan = match_market(
            PairId(1), 42, &market(), vec![a2.clone(), a1.clone(), b.clone()], // note: order vec shuffled
            &owners, &StubPoseidon, false, |_b,_m| [0u8;32]
        );

        // Fill must hit a1 (seq 10) before a2 (seq 11)
        assert_eq!(plan.fills.len(), 2);
        assert_eq!(plan.fills[0].seller_order_id.0, 1);
        assert_eq!(plan.fills[0].fill_qty, 3);
        assert_eq!(plan.fills[1].seller_order_id.0, 2);
        assert_eq!(plan.fills[1].fill_qty, 4);

        // Maker price rule: both fills at 100
        assert!(plan.fills.iter().all(|f| f.price_tick == 100));
    }

    // #[test]
    // fn fifo_same_price_on_bid_side() {
    //     // Two bids @100 with ingest_seq ordering; asks consume FIFO
    //     let b1 = mk_order(1, Side::Bid, 100, 2, 2, 10, 0);
    //     let b2 = mk_order(2, Side::Bid, 100, 4, 4, 11, 0);
    //     let a  = mk_order(3, Side::Ask, 100, 5, 5, 20, 0);

    //     let owners = owners(&[1,2,3]);
    //     let plan = match_market(
    //         PairId(1), 7, &market(), vec![b2.clone(), a.clone(), b1.clone()],
    //         &owners, &StubPoseidon, false, |_b,_m| [0u8;32]
    //     );

    //     assert_eq!(plan.fills.len(), 2);
    //     assert_eq!(plan.fills[0].buyer_order_id.0, 1);
    //     assert_eq!(plan.fills[0].fill_qty, 2);
    //     assert_eq!(plan.fills[1].buyer_order_id.0, 2);
    //     assert_eq!(plan.fills[1].fill_qty, 3);
    // }

    #[test]
    fn multi_level_crossing_and_partials() {
        // Best ask 95 qty 7, next ask 100 qty 8; bid 100 qty 10
        let a1 = mk_order(1, Side::Ask,  95, 7, 7, 10, 0);
        let a2 = mk_order(2, Side::Ask, 100, 8, 8, 11, 0);
        let b  = mk_order(3, Side::Bid, 100,10,10, 12, 0);

        let owners = owners(&[1,2,3]);
        let plan = match_market(
            PairId(1), 9, &market(), vec![a1.clone(), b.clone(), a2.clone()],
            &owners, &StubPoseidon, false, |_b,_m| [0u8;32]
        );

        assert_eq!(plan.fills.len(), 2);
        assert_eq!(plan.fills[0].price_tick, 95);
        assert_eq!(plan.fills[0].fill_qty, 7);
        assert_eq!(plan.fills[1].price_tick, 100);
        assert_eq!(plan.fills[1].fill_qty, 3);

        // Residuals: bid 0, a1 0, a2 5
        let map = plan.residuals.iter().map(|r|(r.order_id.0, r.remaining_after)).collect::<HashMap<_,_>>();
        assert_eq!(map.get(&3), Some(&0));
        assert_eq!(map.get(&1), Some(&0));
        assert_eq!(map.get(&2), Some(&5));
    }

    #[test]
    fn run_to_exhaustion_many_fills() {
        // Stair-step ladder: bid 105(2), 103(3), 101(4); asks 99(3), 101(3), 102(4)
        let b1 = mk_order(1, Side::Bid, 105, 2, 2, 1, 0);
        let b2 = mk_order(2, Side::Bid, 103, 3, 3, 2, 0);
        let b3 = mk_order(3, Side::Bid, 101, 4, 4, 3, 0);

        let a1 = mk_order(4, Side::Ask,  99, 3, 3, 1, 0);
        let a2 = mk_order(5, Side::Ask, 101, 3, 3, 2, 0);
        let a3 = mk_order(6, Side::Ask, 102, 4, 4, 3, 0);

        let owners = owners(&[1,2,3,4,5,6]);
        let plan = match_market(
            PairId(1), 100, &market(),
            vec![b1,b2,b3,a1,a2,a3], &owners, &StubPoseidon, false, |_b,_m| [0u8;32]
        );

        // Ensure we ran multiple fills and fully crossed
        assert!(!plan.fills.is_empty());
        // Best ask 99 should fill first at 99
        assert_eq!(plan.fills[0].price_tick, 99);
        // Final residuals: check no negative and some orders remain plausible
        assert!(plan.residuals.iter().all(|r| r.remaining_after <= r.remaining_before));
    }

    #[test]
    fn match_id_resets_per_call_and_is_monotonic() {
        let m = market();
        let a = mk_order(1, Side::Ask, 100, 5, 5, 1, 0);
        let b = mk_order(2, Side::Bid, 100, 5, 5, 2, 0);
        let owners = owners(&[1,2]);

        let p1 = match_market(PairId(1), 1, &m, vec![a.clone(), b.clone()], &owners, &StubPoseidon, false, |_b,_m| [0u8;32]);
        let p2 = match_market(PairId(1), 2, &m, vec![a, b], &owners, &StubPoseidon, false, |_b,_m| [0u8;32]);

        assert_eq!(p1.fills.len(), 1);
        assert_eq!(p2.fills.len(), 1);
        assert_eq!(p1.fills[0].match_id, 1);
        assert_eq!(p2.fills[0].match_id, 1); // resets each call
    }

    #[test]
    fn pid_changes_with_salt_but_match_id_stable() {
        let m = market();
        let a = mk_order(1, Side::Ask, 100, 5, 5, 1, 0);
        let b = mk_order(2, Side::Bid, 100, 5, 5, 2, 0);
        let owners = owners(&[1,2]);

        let p_no = match_market(PairId(1), 11, &m, vec![a.clone(), b.clone()], &owners, &StubPoseidon, false, |_b,_m| [0u8;32]);
        let p_s  = match_market(PairId(1), 11, &m, vec![a, b], &owners, &StubPoseidon, true, |_b,_m| {
            let mut s = [0u8;32]; s[31]=0xAB; s
        });

        assert_eq!(p_no.fills[0].match_id, 1);
        assert_eq!(p_s.fills[0].match_id, 1);
        assert_ne!(p_no.fills[0].buyer_pid, p_s.fills[0].buyer_pid);
        assert_ne!(p_no.fills[0].seller_pid, p_s.fills[0].seller_pid);
    }

    #[test]
    fn time_bucket_policy_is_max() {
        let a = mk_order(1, Side::Ask, 100, 2, 2, 1, 5);
        let b = mk_order(2, Side::Bid, 100, 2, 2, 2, 7);
        let owners = owners(&[1,2]);

        let p = match_market(PairId(1), 77, &market(), vec![a,b], &owners, &StubPoseidon, false, |_b,_m| [0u8;32]);

        assert_eq!(p.fills.len(), 1);
        assert_eq!(p.fills[0].time_bucket, 7);
    }

    #[test]
    fn exercises_bi_greater_than_ai_split_branch() {
        let ask_first = mk_order(1, Side::Ask,  90, 3, 3, 1, 0);
        let bid_second= mk_order(2, Side::Bid, 100, 3, 3, 1, 0);
        let owners = owners(&[1,2]);
        let p = match_market(PairId(1), 55, &market(), vec![ask_first, bid_second], &owners, &StubPoseidon, false, |_b,_m| [0u8;32]);

        assert_eq!(p.fills.len(), 1);
        assert_eq!(p.fills[0].price_tick, 90); // maker = resting ask
        assert_eq!(p.fills[0].fill_qty, 3);
    }

    #[test]
    fn no_cross_produces_no_fills() {
        let a = mk_order(1, Side::Ask, 101, 5, 5, 1, 0);
        let b = mk_order(2, Side::Bid, 100, 5, 5, 1, 0);
        let owners = owners(&[1,2]);

        let p = match_market(PairId(1), 1, &market(), vec![a,b], &owners, &StubPoseidon, false, |_b,_m| [0u8;32]);
        assert!(p.fills.is_empty());
        assert!(p.residuals.is_empty());
    }
}

// #[test]
// fn heap_tiebreak_fifo_bids() {
//     use std::collections::BinaryHeap;
//     let a = OrderKey { side: Side::Bid, price_tick: 100, ingest_seq: 10 };
//     let b = OrderKey { side: Side::Bid, price_tick: 100, ingest_seq: 11 };

//     #[derive(Clone)]
//     struct Item(OrderKey);
//     impl Eq for Item {}
//     impl PartialEq for Item { fn eq(&self, o:&Self)->bool { self.0.price_tick==o.0.price_tick && self.0.ingest_seq==o.0.ingest_seq && self.0.side==o.0.side } }
//     impl Ord for Item { fn cmp(&self, o:&Self)->std::cmp::Ordering { OrderKey::cmp(&self.0,&o.0) } }
//     impl PartialOrd for Item { fn partial_cmp(&self, o:&Self)->Option<std::cmp::Ordering>{Some(self.cmp(o))} }

//     let mut h = BinaryHeap::new();
//     h.push(Item(b));
//     h.push(Item(a));

//     // top must be ingest_seq 10 (earlier => FIFO)
//     let top = h.pop().unwrap().0;
//     assert_eq!(top.ingest_seq, 10);
// }

