mod build;
mod lookup;
mod storage;

pub use build::{RecursiveGeometryBuilder, frame_recursive_array_extent};
use storage::STORAGE_BYTES;

// moved geometry types
/// Exact geometry facts returned by one generated iterative recursive skip.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecursiveMeasure {
    /// Exact represented width.
    pub consumed: usize,
    /// Deterministic generated structural signature.
    pub shape: u64,
    /// Deepest recursive continuation entered while skipping this root.
    pub nested_depth: u32,
}

/// Compact exact geometry retained for one recursive array.
#[doc(hidden)]
#[derive(Clone)]
pub struct RecursiveGeometry {
    storage: [u8; STORAGE_BYTES],
    meta: [u32; 3],
    kind: u8,
}

impl RecursiveGeometry {
    /// Creates an empty exact geometry that falls back to replay until populated.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            storage: [0; STORAGE_BYTES],
            meta: [0; 3],
            kind: storage::GEOMETRY_REPLAY,
        }
    }

    pub(super) fn reset(&mut self, kind: u8) {
        self.storage.fill(0);
        self.meta = [0; 3];
        self.kind = kind;
    }

    /// Reports the selected exact lookup strategy for diagnostics and measurement.
    #[doc(hidden)]
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self.kind {
            storage::GEOMETRY_FIXED => "fixed",
            storage::GEOMETRY_FORMULA => "exact_formula",
            storage::GEOMETRY_INTERVAL => "interval_events",
            storage::GEOMETRY_RANKED => "ranked_palette",
            storage::GEOMETRY_FACTORIZED => "factorized",
            storage::GEOMETRY_RECURSIVE_SHAPE => "recursive_shape",
            storage::GEOMETRY_PERIODIC => "periodic_palette",
            storage::GEOMETRY_PACKED_RUNS => "packed_runs",
            storage::GEOMETRY_REPLAY => "replay",
            _ => "invalid",
        }
    }
}

impl Default for RecursiveGeometry {
    fn default() -> Self {
        Self::new()
    }
}
