use uuid::Uuid;

use crate::miner::MineRequest;

/// Vérifie qu'un nonce produit un hash avec au moins `target_bits` bits de tête à zéro.
///
/// Le hash est calculé avec blake3 sur la concaténation :
///   seed ‖ tick (LE) ‖ resource_id (bytes) ‖ agent_id (bytes) ‖ nonce (LE)
pub fn pow_valid(mine_request: &MineRequest, nonce: u64) -> bool {
    let hash = pow_hash(
        &mine_request.seed,
        mine_request.tick,
        mine_request.resource_id,
        mine_request.agent_id,
        nonce,
    );
    leading_zero_bits(&hash) >= mine_request.target_bits
}

/// Cherche un nonce valide par force brute en partant de `start_nonce`.
///
/// Teste `batch_size` nonces consécutifs. Retourne `Some(nonce)` si un nonce valide
/// est trouvé, `None` sinon.
///
/// Astuce : chaque thread mineur appelle cette fonction avec un `start_nonce` différent
/// pour paralléliser la recherche.
pub fn pow_search(request: &MineRequest, start_nonce: u64, batch_size: u64) -> Option<u64> {
    for nonce in start_nonce..start_nonce.saturating_add(batch_size) {
        if pow_valid(request, nonce) {
            return Some(nonce);
        }
    }
    None
}

fn pow_hash(seed: &str, tick: u64, resource_id: Uuid, agent_id: Uuid, nonce: u64) -> [u8; 32] {
    use blake3::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(seed.as_bytes());
    hasher.update(&tick.to_le_bytes());
    hasher.update(resource_id.as_bytes());
    hasher.update(agent_id.as_bytes());
    hasher.update(&nonce.to_le_bytes());
    *hasher.finalize().as_bytes()
}

fn leading_zero_bits(bytes: &[u8]) -> u8 {
    let mut count: u8 = 0;
    for b in bytes {
        let lz = b.leading_zeros() as u8;
        count += lz;
        if lz < 8 {
            break;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pow_search_finds_valid_nonce() {
        let mine_request = MineRequest {
            seed: "test_seed".to_owned(),
            tick: 42,
            resource_id: Uuid::new_v4(),
            agent_id: Uuid::new_v4(),
            target_bits: 4, // facile pour un test
        };

        let nonce = pow_search(&mine_request, 0, 100_000)
            .expect("devrait trouver un nonce avec 4 bits en < 100k essais");
        assert!(pow_valid(&mine_request, nonce,));
    }

    #[test]
    fn test_pow_valid_rejects_bad_nonce() {
        let mine_request = MineRequest {
            seed: "test_seed".to_owned(),
            tick: 42,
            resource_id: Uuid::new_v4(),
            agent_id: Uuid::new_v4(),
            target_bits: 32,
        };
        // 32 bits de zéro : extrêmement improbable pour nonce = 0
        assert!(!pow_valid(&mine_request, 0));
    }
}
