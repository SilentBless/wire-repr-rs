//! Helpers for computed wire fields.

/// Returns the number of values in a semantic slice argument.
pub const fn len<T>(values: &[T]) -> usize {
    values.len()
}
