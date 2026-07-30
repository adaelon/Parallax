use std::{error::Error, fmt};

pub const EMBEDDING_MODEL_VERSION: &str = "eam-subword-hash-embedding-v1";
pub const VECTOR_DIMENSIONS: usize = 256;
pub const VECTOR_BYTES: usize = VECTOR_DIMENSIONS * size_of::<i16>();
pub const VECTOR_MIN_SCORE_BPS: u16 = 2_500;

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorEmbedding {
    values: [i16; VECTOR_DIMENSIONS],
}

impl VectorEmbedding {
    #[must_use]
    pub const fn values(&self) -> &[i16; VECTOR_DIMENSIONS] {
        &self.values
    }

    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.values.iter().all(|value| *value == 0)
    }

    #[must_use]
    pub fn to_le_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(VECTOR_BYTES);
        for value in self.values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    /// Restores the fixed-width persisted representation.
    ///
    /// # Errors
    ///
    /// Returns [`EmbeddingError::InvalidByteLength`] when the input is not one
    /// complete v1 vector.
    pub fn from_le_bytes(bytes: &[u8]) -> Result<Self, EmbeddingError> {
        if bytes.len() != VECTOR_BYTES {
            return Err(EmbeddingError::InvalidByteLength);
        }
        let mut values = [0_i16; VECTOR_DIMENSIONS];
        for (value, pair) in values.iter_mut().zip(bytes.chunks_exact(2)) {
            *value = i16::from_le_bytes([pair[0], pair[1]]);
        }
        Ok(Self { values })
    }
}

/// Embeds text with the fully local, versioned G07 subword hashing model.
#[must_use]
pub fn embed_text(text: &str) -> VectorEmbedding {
    let mut values = [0_i16; VECTOR_DIMENSIONS];
    let mut run = String::new();
    let flush = |run: &mut String, values: &mut [i16; VECTOR_DIMENSIONS]| {
        if run.is_empty() {
            return;
        }
        add_feature(values, run.as_bytes(), 3);

        let bounded = format!("^{run}$").chars().collect::<Vec<_>>();
        for width in 3..=5 {
            for feature in bounded.windows(width) {
                let value = feature.iter().collect::<String>();
                add_feature(values, value.as_bytes(), 1);
            }
        }

        if !run.is_ascii() {
            let characters = run.chars().collect::<Vec<_>>();
            for width in 1..=3 {
                for feature in characters.windows(width) {
                    let value = feature.iter().collect::<String>();
                    add_feature(values, value.as_bytes(), 2);
                }
            }
        }
        run.clear();
    };

    for character in text.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() {
            run.push(character);
        } else {
            flush(&mut run, &mut values);
        }
    }
    flush(&mut run, &mut values);
    VectorEmbedding { values }
}

/// Computes exact cosine similarity and quantizes it to basis points.
#[must_use]
pub fn cosine_similarity_bps(left: &VectorEmbedding, right: &VectorEmbedding) -> u16 {
    let mut dot = 0_i64;
    let mut left_norm = 0_u64;
    let mut right_norm = 0_u64;
    for (left, right) in left.values.iter().zip(right.values.iter()) {
        let left = i64::from(*left);
        let right = i64::from(*right);
        dot = dot.saturating_add(left.saturating_mul(right));
        left_norm = left_norm.saturating_add(left.unsigned_abs().saturating_pow(2));
        right_norm = right_norm.saturating_add(right.unsigned_abs().saturating_pow(2));
    }
    if dot <= 0 || left_norm == 0 || right_norm == 0 {
        return 0;
    }
    let denominator = (u128::from(left_norm) * u128::from(right_norm)).isqrt();
    if denominator == 0 {
        return 0;
    }
    let score = (u128::try_from(dot).unwrap_or(0) * 10_000) / denominator;
    u16::try_from(score.min(10_000)).unwrap_or(10_000)
}

fn add_feature(values: &mut [i16; VECTOR_DIMENSIONS], feature: &[u8], weight: i16) {
    let hash = feature.iter().fold(FNV_OFFSET_BASIS, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
    });
    let index = usize::try_from(hash & 0xff).unwrap_or(0);
    let signed_weight = if hash & 0x100 == 0 { weight } else { -weight };
    values[index] = values[index].saturating_add(signed_weight);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbeddingError {
    InvalidByteLength,
}

impl fmt::Display for EmbeddingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("embedding byte length does not match the frozen vector model")
    }
}

impl Error for EmbeddingError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_bytes_and_scores_are_replayable() {
        let first = embed_text("Coordinating Project Aurora");
        let second = embed_text("coordinating project aurora");
        let restored = VectorEmbedding::from_le_bytes(&first.to_le_bytes()).unwrap();

        assert_eq!(first, second);
        assert_eq!(first, restored);
        assert_eq!(cosine_similarity_bps(&first, &second), 10_000);
        assert!(VectorEmbedding::from_le_bytes(&first.to_le_bytes()[..VECTOR_BYTES - 1]).is_err());
    }

    #[test]
    fn blank_input_is_not_a_vector_candidate() {
        let blank = embed_text(" \n\t");
        assert!(blank.is_zero());
        assert_eq!(cosine_similarity_bps(&blank, &embed_text("anything")), 0);
    }
}
