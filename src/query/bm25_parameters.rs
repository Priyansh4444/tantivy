/// Validated query-time BM25 parameters. Index-time norms remain unchanged.
#[derive(Clone, Copy, Debug)]
pub struct Bm25Parameters {
    k1: f32,
    b: f32,
}

/// A parameter outside Lucene 10.4's BM25 constructor domain.
#[derive(Clone, Copy, Debug, thiserror::Error)]
pub enum InvalidBm25Parameters {
    /// Nonfinite or negative term-frequency saturation parameter.
    #[error("k1 must be a nonnegative finite float, got {0}")]
    InvalidK1(f32),
    /// NaN or length-normalization parameter outside zero through one.
    #[error("b must be between zero and one, got {0}")]
    InvalidB(f32),
}

impl From<InvalidBm25Parameters> for crate::TantivyError {
    fn from(error: InvalidBm25Parameters) -> Self {
        Self::InvalidArgument(error.to_string())
    }
}

impl Bm25Parameters {
    /// Lucene's default query parameters.
    pub const DEFAULT: Self = Self { k1: 1.2, b: 0.75 };

    /// Validate parameters without canonicalizing signed zeros.
    pub fn new(k1: f32, b: f32) -> Result<Self, InvalidBm25Parameters> {
        if !k1.is_finite() || k1 < 0.0 {
            return Err(InvalidBm25Parameters::InvalidK1(k1));
        }
        if b.is_nan() || !(0.0..=1.0).contains(&b) {
            return Err(InvalidBm25Parameters::InvalidB(b));
        }
        Ok(Self { k1, b })
    }

    /// Term-frequency saturation parameter, preserving its original float bits.
    pub fn k1(self) -> f32 {
        self.k1
    }

    /// Length-normalization parameter, preserving its original float bits.
    pub fn b(self) -> f32 {
        self.b
    }

    pub(crate) fn is_default_profile(self) -> bool {
        self.k1.to_bits() == Self::DEFAULT.k1.to_bits()
            && self.b.to_bits() == Self::DEFAULT.b.to_bits()
    }
}

impl Default for Bm25Parameters {
    fn default() -> Self {
        Self::DEFAULT
    }
}
