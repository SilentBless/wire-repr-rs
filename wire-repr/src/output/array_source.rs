//! Deferred streaming sources for arrays in detached builders.

use core::marker::PhantomData;

use super::{ChildWriter, Output, WriteError};
use crate::schema::{WireBuilder, WireWrite};

/// Detached builder facade that retains a source for later array emission.
#[doc(hidden)]
pub struct ArrayBuilder<T> {
    marker: PhantomData<fn() -> T>,
}

impl<T> ArrayBuilder<T> {
    /// Creates an empty detached array builder without an output cursor.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            marker: PhantomData,
        }
    }

    /// Retains the source and callback without iterating or building any items.
    #[must_use]
    pub fn extend<Values, Build, Value>(
        self,
        values: Values,
        build: Build,
    ) -> ArraySource<T, Values, Build>
    where
        T: WireBuilder + WireWrite<Value>,
        Values: IntoIterator,
        Build: FnMut(T::Builder, Values::Item) -> Value,
    {
        ArraySource {
            values,
            build,
            marker: self.marker,
        }
    }
}

impl<T> Default for ArrayBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// An owned source and callback consumed once when a detached array is written.
#[doc(hidden)]
pub struct ArraySource<T, Values, Build> {
    values: Values,
    build: Build,
    marker: PhantomData<fn() -> T>,
}

/// Emits a detached counted array directly into its parent's child cursor.
#[doc(hidden)]
pub trait WriteArray<T> {
    /// Schema-specific failure while writing one array item.
    type Error: core::error::Error + 'static;

    /// Consumes the source, emits its items, and returns the emitted item count.
    fn write<O: Output>(
        self,
        writer: &mut ChildWriter<'_, O>,
    ) -> Result<usize, WriteError<Self::Error, O::GrowError>>;
}

impl<T, Values, Build, Value> WriteArray<T> for ArraySource<T, Values, Build>
where
    T: WireBuilder + WireWrite<Value>,
    Values: IntoIterator,
    Build: FnMut(T::Builder, Values::Item) -> Value,
{
    type Error = <T as WireWrite<Value>>::Error;

    #[inline(always)]
    fn write<O: Output>(
        self,
        writer: &mut ChildWriter<'_, O>,
    ) -> Result<usize, WriteError<Self::Error, O::GrowError>> {
        let Self {
            values, mut build, ..
        } = self;
        let mut count = 0usize;
        for value in values {
            count = writer.array_item::<T, _, _>(count, |item| build(item, value))?;
        }
        Ok(count)
    }
}
