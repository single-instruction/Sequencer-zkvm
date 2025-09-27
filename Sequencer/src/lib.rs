pub mod types;      // your Order/Market/Fills types (you already have)
pub mod book;       // your heap/price-time book (you already have)
pub mod r#match;    // your deterministic matching (you already have)
pub mod commit;     // Poseidon-like trait + commit helpers
pub mod encode;     // canonical byte encoders for commitments
pub mod block;      // block structs + builder
pub mod db;         // database traits + Postgres impl

pub use block::{Block, BlockHeader, BlockNumber, BatchId, BlockBuilder};
