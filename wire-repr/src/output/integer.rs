use super::{ChildWriter, Output, OutputError, Writer};

impl<O: Output> Writer<O> {
    /// Expands a reserved one-byte integer prefix to its final encoded bytes.
    #[doc(hidden)]
    pub fn expand_integer_at(
        &mut self,
        offset: usize,
        bytes: &[u8],
    ) -> Result<(), OutputError<O::GrowError>> {
        self.expand_integer_span(offset, self.cursor, bytes, None)?;
        Ok(())
    }

    fn expand_integer_span(
        &mut self,
        offset: usize,
        end: usize,
        bytes: &[u8],
        limit: Option<usize>,
    ) -> Result<usize, OutputError<O::GrowError>> {
        let suffix_start = offset.checked_add(1).ok_or(OutputError::LengthOverflow)?;
        if offset < self.start || suffix_start > end || end > self.cursor {
            return Err(OutputError::Backwards {
                position: offset,
                written: end,
            });
        }
        let added = bytes
            .len()
            .checked_sub(1)
            .ok_or(OutputError::LengthOverflow)?;
        let expanded_end = end.checked_add(added).ok_or(OutputError::LengthOverflow)?;
        if let Some(limit) = limit
            && expanded_end > limit
        {
            return Err(OutputError::ChildOverflow {
                end: expanded_end,
                limit,
            });
        }
        self.ensure(expanded_end, expanded_end.max(self.cursor))?;
        let output = self.output.bytes_mut();
        if added != 0 && suffix_start != end {
            output.copy_within(suffix_start..end, suffix_start + added);
        }
        output[offset..suffix_start + added].copy_from_slice(bytes);
        self.cursor = self.cursor.max(expanded_end);
        Ok(expanded_end)
    }
}

impl<O: Output> ChildWriter<'_, O> {
    /// Expands a reserved one-byte prefix without crossing this child's output boundary.
    #[doc(hidden)]
    pub fn expand_integer_at(
        &mut self,
        offset: usize,
        bytes: &[u8],
    ) -> Result<(), OutputError<O::GrowError>> {
        if offset < self.start {
            return Err(OutputError::Backwards {
                position: offset,
                written: self.start,
            });
        }
        self.cursor = self
            .writer
            .expand_integer_span(offset, self.cursor, bytes, self.limit)?;
        Ok(())
    }
}
