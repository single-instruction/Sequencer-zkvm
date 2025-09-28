pub mod commit;     // Poseidon-like trait + commit helpers
pub mod encode;     // canonical byte encoders for commitments
pub mod block;      // block structs + builder
pub mod db;         // database traits + Postgres impl
pub mod match_loop;
pub mod mempool;
pub mod state;

pub use block::{Block, BlockHeader, BlockNumber, BatchId, BlockBuilder};
pub use engine::types::*;
pub use engine::r#match;
