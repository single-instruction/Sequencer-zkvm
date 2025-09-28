// use ark_bn254::{Bn254, Fr};
// use ark_groth16::{prepare_verifying_key, verify_proof, PreparedVerifyingKey, Proof, VerifyingKey};
// use ark_serialize::{CanonicalDeserialize};
// use serde::{Deserialize, Serialize};
// use thiserror::Error;
// use std::{sync::Arc};
// use tokio::sync::RwLock;

// #[derive(Debug, Error)]
// pub enum MempoolError {
//     #[error("bad input: {0}")]
//     BadInput(&'static str),
//     #[error("vk deserialization failed")]
//     VkDeserialize,
//     #[error("proof parsing failed: {0}")]
//     ProofParse(&'static str),
//     #[error("public signals length mismatch: expected {expected}, got {got}")]
//     PublicLen { expected: usize, got: usize },
//     #[error("groth16 verification failed")]
//     VerifyFailed,
// }

// #[derive(Debug, Deserialize)]
// pub struct SubmitOrderWithProof {
//     pub orderParams: OrderParams,
//     pub proof: SnarkJsProof,        // snarkjs proof (see below)
//     pub publicSignals: Vec<String>, // decimal strings
// }

// #[derive(Debug, Deserialize)]
// pub struct OrderParams {
//     pub pairIdHash: String,  // "0x.."
//     pub side: u8,
//     pub priceTick: u64,
//     pub amount: u64,
//     pub timeBucket: u32,
//     pub nonce: u64,
//     pub structHash: String,  // "0x.."
// }

// #[derive(Debug, Deserialize)]
// #[serde(untagged)]
// pub enum SnarkJsProof {
//     Flat {
//         pi_a: [String; 2],
//         pi_b: [[String; 2]; 2],
//         pi_c: [String; 2],
//     },
//     Nested {
//         proof: Inner,
//     },
// }

// #[derive(Debug, Deserialize)]
// pub struct Inner {
//     pub A: [String; 2],
//     pub B: [[String; 2]; 2],
//     pub C: [String; 2],
// }

// /// In-memory queue item
// #[derive(Debug, Clone, Serialize)]
// pub struct QueuedOrder {
//     pub order_id: u64,
//     pub ingest_seq: u64,
//     pub struct_hash: String,
//     pub nullifier: String,
//     pub order_hash: String,
// }

// /// Simple mempool with a prepared VK and an in-memory queue
// #[derive(Clone)]
// pub struct Mempool {
//     pvk: Arc<PreparedVerifyingKey<Bn254>>,
//     queue: Arc<RwLock<Vec<QueuedOrder>>>,
// }

// impl Mempool {
//     /// Load a verifying key. You have options:
//     ///   - include_bytes! a bincode-serialized arkworks VK (recommended for prod)
//     ///   - or load from disk at startup and pass bytes here
//     pub fn from_vk_bytes(vk_bytes: &[u8]) -> Result<Self, MempoolError> {
//         let vk = VerifyingKey::<Bn254>::deserialize_compressed(vk_bytes)
//             .map_err(|_| MempoolError::VkDeserialize)?;
//         Ok(Self {
//             pvk: Arc::new(prepare_verifying_key(&vk)),
//             queue: Arc::new(RwLock::new(Vec::new())),
//         })
//     }

//     /// Main entry: validate + verify + enqueue
//     pub async fn submit(&self, req: SubmitOrderWithProof) -> Result<QueuedOrder, MempoolError> {
//         // --- cheap semantic checks first ---
//         if req.orderParams.amount == 0 || req.orderParams.priceTick == 0 {
//             return Err(MempoolError::BadInput("amount/priceTick must be > 0"));
//         }
//         if req.orderParams.side > 1 {
//             return Err(MempoolError::BadInput("side must be 0|1"));
//         }

//         // --- map snarkjs proof/public inputs to arkworks types ---
//         let proof = parse_snarkjs_proof(&req.proof)?;
//         if req.publicSignals.len() != 3 {
//             return Err(MempoolError::PublicLen { expected: 3, got: req.publicSignals.len() });
//         }

//         // we assume publicSignals = [structHash, nullifier, orderHash] as decimals (snarkjs default).
//         // if yours differs, adjust mapping here.
//         let publics: Vec<Fr> = req.publicSignals.iter()
//             .map(|s| dec_to_fr(s))
//             .collect::<Result<_,_>>()
//             .map_err(|_| MempoolError::BadInput("invalid public signal"))?;

//         // --- optional server-side consistency checks with payload ---
//         // e.g., ensure structHash in payload equals the one in publicSignals[0]
//         // Convert hex "0x.." -> Fr and compare
//         {
//             let ph = hex_to_fr(&req.orderParams.structHash)
//                 .ok_or(MempoolError::BadInput("bad structHash hex"))?;
//             if ph != publics[0] {
//                 return Err(MempoolError::BadInput("structHash mismatch vs publicSignals[0]"));
//             }
//         }

//         // --- Groth16 verification ---
//         let ok = verify_proof(&self.pvk, &proof, &publics).map_err(|_| MempoolError::VerifyFailed)?;
//         if !ok { return Err(MempoolError::VerifyFailed); }

//         // --- enqueue (mock ids; replace with DB sequence if you like) ---
//         let q = QueuedOrder {
//             order_id: monotonic_id(),
//             ingest_seq: monotonic_id(),
//             struct_hash: req.orderParams.structHash,
//             nullifier: fr_to_dec(&publics[1]),
//             order_hash: fr_to_dec(&publics[2]),
//         };

//         self.queue.write().await.push(q.clone());
//         Ok(q)
//     }

//     pub async fn pending_len(&self) -> usize {
//         self.queue.read().await.len()
//     }
// }

// /// ---- helpers ----

// fn dec_to_fr(s: &str) -> Result<Fr, ()> {
//     // snarkjs gives decimal strings; reduce mod Fr::MODULUS
//     let bi = ark_std::num::BigUint::parse_bytes(s.as_bytes(), 10).ok_or(())?;
//     Ok(biguint_to_fr(&bi))
// }

// fn hex_to_fr(s: &str) -> Option<Fr> {
//     let s = s.strip_prefix("0x").unwrap_or(s);
//     let bytes = hex::decode(s).ok()?;
//     // interpret bytes as big-endian integer
//     let bi = ark_std::num::BigUint::from_bytes_be(&bytes);
//     Some(biguint_to_fr(&bi))
// }

// fn biguint_to_fr(bi: &ark_std::num::BigUint) -> Fr {
//     use ark_std::num::BigUint as BigU;
//     use ark_ff::PrimeField;
//     // reduce BigUint mod field modulus
//     let modulus = Fr::MODULUS.into_biguint();
//     let reduced = bi % &modulus;
//     Fr::from_be_bytes_mod_order(&reduced.to_bytes_be())
// }

// /// Turn ark field back to decimal (for API responses / logs)
// fn fr_to_dec(x: &Fr) -> String {
//     use ark_ff::PrimeField;
//     let be = x.into_bigint().to_bytes_be();
//     ark_std::num::BigUint::from_bytes_be(&be).to_str_radix(10)
// }

// /// Parse snarkjs JSON into ark_groth16::Proof
// fn parse_snarkjs_proof(p: &SnarkJsProof) -> Result<Proof<Bn254>, MempoolError> {
//     // snarkjs uses affine coords in decimals; each limb is in Fp
//     let (a, b, c) = match p {
//         SnarkJsProof::Flat { pi_a, pi_b, pi_c } => (pi_a.clone(), pi_b.clone(), pi_c.clone()),
//         SnarkJsProof::Nested { proof } => (proof.A.clone(), proof.B.clone(), proof.C.clone()),
//     };

//     // groth16 expects:
//     //  A in G1, B in G2, C in G1. arkworks Proof::new takes projective, but we can use from coordinates helper.
//     // We rely on arkworks' helper from raw bigints via serde? Not provided; build via snarkjs pairing order:
//     // However, ark_groth16::Proof has fields A (G1), B (G2), C (G1) and implements CanonicalDeserialize,
//     // but we construct via strings -> bigints -> group points using ark_ec API is non-trivial.
//     //
//     // Simpler path: snarkjs JSON points are in affine coordinates over BN254 and match arkworks order.
//     // We can leverage the `serde_json` "groth16 proof export" that's bincode-compatible ONLY if you produced it with arkworks.
//     // Since you have snarkjs, we’ll use ark-circom’s conversion or manual parsing. To keep this file self-contained,
//     // use the built-in `Proof::<Bn254>::from_json`-like routine:

//     use ark_ec::{AffineRepr, CurveGroup};
//     use ark_bn254::{G1Affine, G1Projective, G2Affine, G2Projective};

//     fn s_to_f(s: &str) -> Result<ark_bn254::Fq, ()> {
//         let bi = ark_std::num::BigUint::parse_bytes(s.as_bytes(), 10).ok_or(())?;
//         use ark_ff::PrimeField;
//         let modulus = <ark_bn254::Fq as ark_ff::PrimeField>::MODULUS.into_biguint();
//         let reduced = bi % &modulus;
//         Ok(ark_bn254::Fq::from_be_bytes_mod_order(&reduced.to_bytes_be()))
//     }

//     // G1: (x, y), G2: (x_c0,x_c1) pairs
//     let a_x = s_to_f(&a[0]).map_err(|_| MempoolError::ProofParse("A.x"))?;
//     let a_y = s_to_f(&a[1]).map_err(|_| MempoolError::ProofParse("A.y"))?;
//     let a_aff = G1Affine::new_unchecked(a_x, a_y);
//     let a_proj: G1Projective = a_aff.into();

//     let b_x_c0 = s_to_f(&b[0][0]).map_err(|_| MempoolError::ProofParse("B.x.c0"))?;
//     let b_x_c1 = s_to_f(&b[0][1]).map_err(|_| MempoolError::ProofParse("B.x.c1"))?;
//     let b_y_c0 = s_to_f(&b[1][0]).map_err(|_| MempoolError::ProofParse("B.y.c0"))?;
//     let b_y_c1 = s_to_f(&b[1][1]).map_err(|_| MempoolError::ProofParse("B.y.c1"))?;
//     let b_aff = G2Affine::new_unchecked(
//         ark_bn254::Fq2::new(b_x_c0, b_x_c1),
//         ark_bn254::Fq2::new(b_y_c0, b_y_c1),
//     );
//     let b_proj: G2Projective = b_aff.into();

//     let c_x = s_to_f(&c[0]).map_err(|_| MempoolError::ProofParse("C.x"))?;
//     let c_y = s_to_f(&c[1]).map_err(|_| MempoolError::ProofParse("C.y"))?;
//     let c_aff = G1Affine::new_unchecked(c_x, c_y);
//     let c_proj: G1Projective = c_aff.into();

//     Ok(Proof::<Bn254> { a: a_proj.into_affine(), b: b_proj.into_affine(), c: c_proj.into_affine() })
// }

// /// ultra-silly monotonic id generator (threadlocal); replace with DB/atomic counter as needed
// fn monotonic_id() -> u64 {
//     use std::sync::atomic::{AtomicU64, Ordering};
//     static ID: AtomicU64 = AtomicU64::new(41);
//     ID.fetch_add(1, Ordering::Relaxed)
// }
