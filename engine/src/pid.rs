use crate::types::PkHash;

pub trait Poseidon32 {
    fn hash_many32(&self, domain_tag: u64, elems: &[[u8;32]]) -> [u8;32];
}

pub struct StubPoseidon;
impl Poseidon32 for StubPoseidon {
    fn hash_many32(&self, domain_tag: u64, elems: &[[u8;32]]) -> [u8;32] {
        let mut out = [0u8; 32];
        out[..8].copy_from_slice(&domain_tag.to_be_bytes());
        for e in elems {
            for i in 0..32 { out[i] ^= e[i]; }
        }
        out
    }
}

/// Domain separators ( both host and SP1)
pub const DS_PID: u64 = 0x_7069645f00000001;

/// Derive a per-fill PID.
///  If you don't use fill_salt, pass None.
pub fn derive_pid<H: Poseidon32>(
    h: &H,
    pk_hash: &PkHash,
    batch_id: u64,
    match_id: u64,
    fill_salt: Option<[u8;32]>,
) -> [u8;32] {
    let mut b = [0u8;32];
    b[24..32].copy_from_slice(&batch_id.to_be_bytes());
    let mut m = [0u8;32];
    m[24..32].copy_from_slice(&match_id.to_be_bytes());

    let elems = if let Some(salt) = fill_salt {
        vec![*pk_hash, b, m, salt]
    } else {
        vec![*pk_hash, b, m]
    };
    h.hash_many32(DS_PID, &elems)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pid_changes_with_salt_and_ids() {
        let h = super::StubPoseidon;
        let pk: [u8;32] = [0x11;32];

        let p1 = super::derive_pid(&h, &pk, 7, 1, None);
        let p2 = super::derive_pid(&h, &pk, 7, 2, None);
        assert_ne!(p1, p2, "different match_id must change pid");

        let salt = [0xAB;32];
        let p3 = super::derive_pid(&h, &pk, 7, 1, Some(salt));
        assert_ne!(p1, p3, "adding salt must change pid");

        let p4 = super::derive_pid(&h, &pk, 7, 1, Some(salt));
        assert_eq!(p3, p4, "deterministic with same inputs");
    }
}
