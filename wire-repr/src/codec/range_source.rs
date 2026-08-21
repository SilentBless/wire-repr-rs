//! Range-source conversion contract.

use super::FixedCodec;

/// Performs checked bidirectional structural conversion between a decoded fixed source
/// representation and byte geometry.
///
/// A range uses the geometry as either a relative byte length (`bytes(source)`) or an
/// exclusive endpoint relative to representation byte zero (`bytes_to(source)`). Generated
/// parsers call [`Self::to_bytes`] at the consuming range before their existing checked
/// range and input-bounds logic. During [`crate::PreparedLayout`] preparation, generated
/// builders derive required geometry, require shared sources to agree on that geometry, and
/// call [`Self::from_bytes`] once for each source before planning its physical codec. Commit
/// only writes the prepared plan and remains capacity-only and atomic.
///
/// Supported source values and byte geometries must round-trip coherently. Implementations
/// must use checked arithmetic and return an explicit error for structural underflow,
/// alignment, or encoded-field bounds they cannot convert. Unrelated protocol policy
/// remains consumer-owned.
///
/// Macro adapters are supported only on direct built-in integer fixed fields that physically
/// precede at least one range. This is a current hard ownership boundary: custom
/// [`FixedCodec`] values or plans can require self-referential prepared storage. The
/// adapter does not create a geometry getter; the ordinary source getter remains the raw
/// wire integer. Unsigned bit projections may coexist with an adapter and read that whole
/// integer; the adapter also consumes and returns the whole packed value.
pub trait RangeSource<C: FixedCodec> {
    /// Error returned when converting between the fixed representation and byte geometry.
    type Error: core::fmt::Debug;

    /// Converts a decoded fixed source value to a byte length or absolute endpoint.
    fn to_bytes(value: C::Value<'_>) -> Result<usize, Self::Error>;

    /// Converts a byte length or absolute endpoint to an owned fixed source value.
    fn from_bytes(bytes: usize) -> Result<C::Value<'static>, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::RangeSource;
    use crate::{BeU16, FixedCodec, U8};

    struct U8Length;

    impl RangeSource<U8> for U8Length {
        type Error = ();

        fn to_bytes(value: <U8 as FixedCodec>::Value<'_>) -> Result<usize, Self::Error> {
            Ok(usize::from(value))
        }

        fn from_bytes(bytes: usize) -> Result<<U8 as FixedCodec>::Value<'static>, Self::Error> {
            u8::try_from(bytes).map_err(|_| ())
        }
    }

    struct BeU16Length;

    impl RangeSource<BeU16> for BeU16Length {
        type Error = ();

        fn to_bytes(value: <BeU16 as FixedCodec>::Value<'_>) -> Result<usize, Self::Error> {
            Ok(usize::from(value))
        }

        fn from_bytes(bytes: usize) -> Result<<BeU16 as FixedCodec>::Value<'static>, Self::Error> {
            u16::try_from(bytes).map_err(|_| ())
        }
    }

    #[test]
    fn local_integer_adapters_need_no_allocation() {
        assert_eq!(U8Length::to_bytes(7), Ok(7));
        assert_eq!(U8Length::from_bytes(255), Ok(255));
        assert_eq!(BeU16Length::to_bytes(0x1234), Ok(0x1234));
        assert_eq!(BeU16Length::from_bytes(0xffff), Ok(0xffff));
    }
}
