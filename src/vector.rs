//! Vector serialization + similarity. Embeddings are stored as little-endian
//! `f32` BLOBs, L2-normalized at write time so cosine similarity reduces to a
//! dot product at query time.

/// Pack a vector into a little-endian `f32` byte blob.
pub fn to_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

/// Unpack a little-endian `f32` byte blob. Trailing bytes (if any) are ignored.
pub fn from_bytes(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// L2-normalize in place. A zero vector is left untouched.
pub fn normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// Dot product. For two normalized vectors this is cosine similarity.
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_bytes() {
        let v = vec![1.0f32, -2.5, 0.0, 3.25];
        assert_eq!(from_bytes(&to_bytes(&v)), v);
    }

    #[test]
    fn normalized_self_dot_is_one() {
        let mut v = vec![3.0f32, 4.0];
        normalize(&mut v);
        assert!((dot(&v, &v) - 1.0).abs() < 1e-6);
    }
}
