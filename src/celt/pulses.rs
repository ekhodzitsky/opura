//! Classic CELT CWRS/PVQ integer pulse-vector decoding.
//!
//! The symmetric U recurrence counts codewords; ordering follows original
//! cwrs.c. Compile-time capped rows avoid allocation and overflowing counts.
//! This primitive does not normalize/rotate vectors or decode complete bands.
//! Copyright (c) 2007-2008 CSIRO; 2007-2009 Xiph.Org Foundation;
//! 2007-2009 Timothy B. Terriberry. See THIRD_PARTY_NOTICES.md.
use crate::range::{RangeDecoder, RangeError};
use std::fmt;

/// Invalid pulse-codebook configuration, index or entropy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PulseError {
    /// Dimensions must be 2..176 and pulse count 0..128.
    InvalidGeometry,
    /// The codebook has more than u32::MAX entries.
    CardinalityOverflow,
    /// Index must be strictly less than the codebook cardinality.
    InvalidIndex,
    /// Range or raw-bit decoding failed.
    Entropy(RangeError),
}
impl fmt::Display for PulseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidGeometry => {
                f.write_str("CELT pulse codebook requires 2..176 dimensions and 0..128 pulses")
            }
            Self::CardinalityOverflow => {
                f.write_str("CELT pulse codebook exceeds 32-bit cardinality")
            }
            Self::InvalidIndex => f.write_str("CELT pulse index exceeds codebook cardinality"),
            Self::Entropy(e) => write!(f, "CELT pulse entropy: {e}"),
        }
    }
}
impl std::error::Error for PulseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Entropy(e) => Some(e),
            _ => None,
        }
    }
}
impl From<RangeError> for PulseError {
    fn from(e: RangeError) -> Self {
        Self::Entropy(e)
    }
}

/// Fixed-storage integer vector; sum of absolute values equals codebook pulses.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PulseVector {
    values: [i16; 176],
    length: usize,
    energy: u32,
}
impl PulseVector {
    /// Signed integer pulses in reference coordinate order.
    pub fn values(&self) -> &[i16] {
        &self.values[..self.length]
    }
    /// Sum of squared integer coordinates, at most 16384.
    pub fn squared_norm(&self) -> u32 {
        self.energy
    }
}

/// Validated immutable algebraic pulse codebook with a 32-bit cardinality.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PulseCodebook {
    dimensions: usize,
    pulses: usize,
    cardinality: u32,
}
impl PulseCodebook {
    /// Validate dimensions/pulses and count signed pulse combinations.
    /// Zero pulses produces the single all-zero vector without entropy bits.
    pub fn new(dimensions: usize, pulses: usize) -> Result<Self, PulseError> {
        if !(2..=176).contains(&dimensions) || pulses > 128 {
            return Err(PulseError::InvalidGeometry);
        }
        let cardinality = if pulses == 0 {
            1
        } else {
            u(dimensions, pulses)
                .checked_add(u(dimensions, pulses + 1))
                .ok_or(PulseError::CardinalityOverflow)?
        };
        Ok(Self {
            dimensions,
            pulses,
            cardinality,
        })
    }
    /// Integer coordinate count.
    pub fn dimensions(self) -> usize {
        self.dimensions
    }
    /// Sum of absolute coordinate values.
    pub fn pulses(self) -> usize {
        self.pulses
    }
    /// Number of representable vectors; valid indices are 0..cardinality.
    pub fn cardinality(self) -> u32 {
        self.cardinality
    }
    /// Decode a codebook index without consuming entropy.
    /// At most dimensions+pulses iterations of the coordinate/search loops.
    pub fn decode_index(self, mut index: u32) -> Result<PulseVector, PulseError> {
        if index >= self.cardinality {
            return Err(PulseError::InvalidIndex);
        }
        let mut result = PulseVector {
            values: [0; 176],
            length: self.dimensions,
            energy: 0,
        };
        let mut pulses = self.pulses;
        for (coordinate, n) in (1..=self.dimensions).rev().enumerate() {
            let boundary = u(n, pulses + 1);
            let negative = index >= boundary;
            if negative {
                index -= boundary;
            }
            let before = pulses;
            while u(n, pulses) > index {
                pulses -= 1;
            }
            index -= u(n, pulses);
            let magnitude = (before - pulses) as i16;
            let value = if negative { -magnitude } else { magnitude };
            result.values[coordinate] = value;
            result.energy += (i32::from(value) * i32::from(value)) as u32;
        }
        Ok(result)
    }
    /// Decode a uniform codebook index and its pulse vector atomically.
    /// Range input may be zero-extended; callers enforce codec bit budgets.
    /// Large codebooks also need physically present raw suffix bits.
    pub fn decode(self, entropy: &mut RangeDecoder<'_>) -> Result<PulseVector, PulseError> {
        let mut next = entropy.clone();
        let index = if self.cardinality == 1 {
            0
        } else {
            next.decode_uint(self.cardinality)?
        };
        let result = self.decode_index(index)?;
        *entropy = next;
        Ok(result)
    }
}

// U(n,k) is symmetric, with U(0,0)=1 and U(0,k)=U(n,0)=0 otherwise.
// All codebooks fitting u32 have min(n,k+1)<=14. Larger rows can return the
// cap immediately: both U terms are positive, so checked V addition rejects.
// Capping entries cannot change any representable codebook's ordering.
const U: [[u32; 177]; 15] = make_u();
const fn make_u() -> [[u32; 177]; 15] {
    let mut result = [[0u32; 177]; 15];
    result[0][0] = 1;
    let mut n = 1;
    while n < 15 {
        let mut k = 1;
        while k < 177 {
            result[n][k] = result[n - 1][k]
                .saturating_add(result[n][k - 1])
                .saturating_add(result[n - 1][k - 1]);
            k += 1;
        }
        n += 1;
    }
    result
}
fn u(n: usize, k: usize) -> u32 {
    if n.min(k) >= 15 {
        u32::MAX
    } else {
        U[n.min(k)][n.max(k)]
    }
}
