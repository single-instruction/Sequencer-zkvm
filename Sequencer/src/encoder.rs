use crate::types::*;

#[inline] fn le64(x: u64) -> [u8;8] { x.to_le_bytes() }
#[inline] fn le32(x: u32) -> [u8;4] { x.to_le_bytes() }
#[inline] fn le16(x: u16) -> [u8;2] { x.to_le_bytes() }

pub fn encode_order(o: &Order) -> Vec<u8> {
    let mut v = Vec::with_capacity(8*9 + 32);
    v.extend_from_slice(&le64(o.order_id.0));
    v.extend_from_slice(&o.order_hash);
    v.extend_from_slice(&le64(o.pair_id.0 as u64));
    v.extend_from_slice(&le64(match o.side { Side::Bid=>0, Side::Ask=>1 }));
    v.extend_from_slice(&le64(o.price_tick));
    v.extend_from_slice(&le64(o.amount));
    v.extend_from_slice(&le64(o.remaining));
    v.extend_from_slice(&le32(o.time_bucket));
    v.extend_from_slice(&le64(o.nonce));
    v.extend_from_slice(&le64(o.ingest_seq));
    v
}

pub fn encode_fill(f: &FillDraft) -> Vec<u8> {
    let mut v = Vec::with_capacity(8*10 + 32*6);
    v.extend_from_slice(&le64(f.batch_id));
    v.extend_from_slice(&le64(f.match_id));
    v.extend_from_slice(&le64(f.pair_id.0 as u64));
    v.extend_from_slice(&le64(f.price_tick));
    v.extend_from_slice(&le64(f.fill_qty));
    v.extend_from_slice(&le32(f.time_bucket));
    v.extend_from_slice(&le64(f.buyer_order_id.0));
    v.extend_from_slice(&le64(f.seller_order_id.0));
    v.extend_from_slice(&f.buyer_order_hash);
    v.extend_from_slice(&f.seller_order_hash);
    v.extend_from_slice(&f.buyer_pid);
    v.extend_from_slice(&f.seller_pid);
    v.extend_from_slice(&le16(f.fee_bps));
    if let Some(s) = f.fill_salt { v.extend_from_slice(&s); }
    v
}

pub fn encode_market(m: &MarketParams) -> Vec<u8> {
    let mut v = Vec::with_capacity(8*6);
    v.extend_from_slice(&le64(m.pair_id.0 as u64));
    v.extend_from_slice(&le64(m.price_tick));
    v.extend_from_slice(&le64(m.size_step));
    v.extend_from_slice(&m.notional_min.to_le_bytes()); // 16 bytes if you store as u128
    v.extend_from_slice(&m.notional_max.to_le_bytes());
    v.extend_from_slice(&le16(m.maker_bps));
    v.extend_from_slice(&le16(m.taker_bps));
    v.extend_from_slice(&le16(match m.status {
        MarketStatus::Active=>0, MarketStatus::Paused=>1,
        MarketStatus::CancelOnly=>2, MarketStatus::Delisted=>3
    }));
    v
}
