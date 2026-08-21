//! Runtime contracts for selecting tagged representations.

/// Resolves and encodes a format-specific tagged discriminant.
///
/// The implementor may use state observed through `self`, such as a negotiated
/// format version. This trait only maps a raw discriminant to a semantic case;
/// it does not parse or encode a case body.
pub trait Discriminant<Raw, Case> {
    /// The error returned when discriminant resolution or encoding fails.
    type Error: core::fmt::Debug;

    /// Resolves a raw discriminant into its semantic case.
    ///
    /// `Ok(None)` reports an unrecognized raw tag separately from a resolver failure.
    fn resolve(&self, raw: Raw) -> Result<Option<Case>, Self::Error>;

    /// Encodes a semantic case as its raw discriminant.
    fn encode(&self, case: Case) -> Result<Raw, Self::Error>;
}

/// Policy for the body of an unrecognized tagged case.
///
/// This policy never guesses a body boundary or attempts to resynchronize parsing.
/// [`Reject`](Self::Reject) is the default. [`Exact`](Self::Exact) accepts exactly a
/// caller-supplied body length, and [`Remainder`](Self::Remainder) consumes the supplied
/// remainder.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum UnknownBody {
    /// Reject the unrecognized tagged case.
    #[default]
    Reject,
    /// Accept an unrecognized body with exactly this caller-supplied length.
    Exact(usize),
    /// Accept an unrecognized body by consuming the supplied remainder.
    Remainder,
}

#[cfg(test)]
mod tests {
    use super::{Discriminant, UnknownBody};

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    enum Case {
        First,
        Second,
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    enum MappingError {
        UnsupportedVersion,
    }

    struct VersionedMapping {
        version: u8,
    }

    impl Discriminant<u8, Case> for VersionedMapping {
        type Error = MappingError;

        fn resolve(&self, raw: u8) -> Result<Option<Case>, Self::Error> {
            match (self.version, raw) {
                (1, 1) | (2, 16) => Ok(Some(Case::First)),
                (1, 2) | (2, 32) => Ok(Some(Case::Second)),
                (1 | 2, _) => Ok(None),
                _ => Err(MappingError::UnsupportedVersion),
            }
        }

        fn encode(&self, case: Case) -> Result<u8, Self::Error> {
            match (self.version, case) {
                (1, Case::First) => Ok(1),
                (1, Case::Second) => Ok(2),
                (2, Case::First) => Ok(16),
                (2, Case::Second) => Ok(32),
                _ => Err(MappingError::UnsupportedVersion),
            }
        }
    }

    #[test]
    fn discriminant_mapping_can_use_runtime_state() {
        let version_one = VersionedMapping { version: 1 };
        let version_two = VersionedMapping { version: 2 };

        assert_eq!(version_one.resolve(1), Ok(Some(Case::First)));
        assert_eq!(version_two.resolve(16), Ok(Some(Case::First)));
        assert_eq!(version_one.resolve(16), Ok(None));
        assert_eq!(
            VersionedMapping { version: 0 }.resolve(1),
            Err(MappingError::UnsupportedVersion)
        );
    }

    #[test]
    fn discriminant_mapping_is_inverse_for_each_version() {
        for mapping in [
            VersionedMapping { version: 1 },
            VersionedMapping { version: 2 },
        ] {
            for case in [Case::First, Case::Second] {
                assert_eq!(
                    mapping.resolve(mapping.encode(case).unwrap()),
                    Ok(Some(case))
                );
            }
        }
    }

    #[test]
    fn unknown_body_defaults_to_rejection() {
        assert_eq!(UnknownBody::default(), UnknownBody::Reject);
    }

    #[test]
    fn unknown_body_provides_exact_and_remainder_policies() {
        assert_eq!(UnknownBody::Exact(12), UnknownBody::Exact(12));
        assert_eq!(UnknownBody::Remainder, UnknownBody::Remainder);
    }
}
