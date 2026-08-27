use super::RecursiveGeometry;
use super::storage::*;
use crate::ArrayError;
use crate::recursive::{FlattenRecursiveError, RecursiveDepth, RecursiveError, RecursiveFrame};

// moved exact lookup
impl RecursiveGeometry {
    #[inline(always)]
    pub(in crate::recursive) fn span<C, Slot, const DEPTH: usize>(
        &self,
        input: &[u8],
        count: usize,
        offset: usize,
        depth: RecursiveDepth,
        requested: usize,
    ) -> Result<Option<core::ops::Range<usize>>, ArrayError<RecursiveError>>
    where
        C: RecursiveFrame<Slot>,
    {
        if requested >= count {
            return Ok(None);
        }
        let direct = match self.kind {
            GEOMETRY_FIXED => {
                let width = self.meta[0] as usize;
                direct_range(requested.checked_mul(width), width, requested, input.len())?
            }
            GEOMETRY_FORMULA => {
                let base = i128::from(self.meta[0]);
                let slope = i128::from(self.meta[1] as i32);
                let item = requested as i128;
                let start = item.checked_mul(base).and_then(|base_prefix| {
                    slope
                        .checked_mul(item)?
                        .checked_mul(item - 1)?
                        .checked_div(2)?
                        .checked_add(base_prefix)
                });
                let width = slope
                    .checked_mul(item)
                    .and_then(|delta| base.checked_add(delta));
                direct_signed_range(start.unwrap_or(i128::MIN), width, requested, input.len())?
            }
            GEOMETRY_INTERVAL => self.interval_span(requested, input.len())?,
            GEOMETRY_RANKED => self.ranked_span(requested, input.len())?,
            GEOMETRY_FACTORIZED => {
                let start = self.factor_prefix(requested)?;
                let end = self.factor_prefix(requested + 1)?;
                let width = end.checked_sub(start).ok_or(ArrayError::InvalidExtent {
                    index: requested,
                    consumed: usize::MAX,
                    available: input.len(),
                })?;
                direct_range(Some(start), width, requested, input.len())?
            }
            GEOMETRY_RECURSIVE_SHAPE => self.shape_span(requested, input.len())?,
            GEOMETRY_PERIODIC => self.periodic_span(requested, input.len())?,
            GEOMETRY_PACKED_RUNS => self.packed_span(requested, input.len())?,
            GEOMETRY_REPLAY => None,
            _ => unreachable!("generated geometry kind"),
        };
        if direct.is_some() {
            return Ok(direct);
        }
        replay_span::<C, Slot, DEPTH>(input, offset, depth, requested)
    }

    #[inline(always)]
    fn interval_span(
        &self,
        requested: usize,
        available: usize,
    ) -> Result<Option<core::ops::Range<usize>>, ArrayError<RecursiveError>> {
        let segments = self.meta[0] as usize;
        let mut selected = 0usize;
        for segment in 1..segments {
            if get_u32(&self.storage, segment * 12) as usize > requested {
                break;
            }
            selected = segment;
        }
        let base = selected * 12;
        let first_item = get_u32(&self.storage, base) as usize;
        let first_byte = get_u32(&self.storage, base + 4) as usize;
        let width = get_u32(&self.storage, base + 8) as usize;
        let start = first_byte.checked_add((requested - first_item).checked_mul(width).ok_or(
            ArrayError::InvalidExtent {
                index: requested,
                consumed: usize::MAX,
                available,
            },
        )?);
        direct_range(start, width, requested, available)
    }

    #[inline(always)]
    fn ranked_span(
        &self,
        requested: usize,
        available: usize,
    ) -> Result<Option<core::ops::Range<usize>>, ArrayError<RecursiveError>> {
        let block = requested / RANKED_BLOCK;
        let first = block * RANKED_BLOCK;
        let mut start = get_u32(&self.storage, RANKED_PREFIXES + block * 4) as usize;
        for item in first..requested {
            let class = get_code(&self.storage, RANKED_CODES, item);
            start = start
                .checked_add(get_u16(&self.storage, RANKED_PALETTE + class * 2) as usize)
                .ok_or(ArrayError::InvalidExtent {
                    index: requested,
                    consumed: usize::MAX,
                    available,
                })?;
        }
        let class = get_code(&self.storage, RANKED_CODES, requested);
        let width = get_u16(&self.storage, RANKED_PALETTE + class * 2) as usize;
        direct_range(Some(start), width, requested, available)
    }

    #[inline(always)]
    fn factor_prefix(&self, items: usize) -> Result<usize, ArrayError<RecursiveError>> {
        let mut prefix = i128::from(self.meta[0]) * items as i128;
        for axis in 0..FACTOR_LENGTHS.len() {
            let block = FACTOR_BLOCKS[axis];
            let classes = FACTOR_LENGTHS[axis];
            let blocks = items / block;
            let tail = items % block;
            let cycles = blocks / classes;
            let class = blocks % classes;
            let mut total = 0i128;
            let mut before = 0i128;
            for component in 0..classes {
                let value = i128::from(factor_component(&self.storage, axis, component));
                total += value;
                if component < class {
                    before += value;
                }
            }
            let value = i128::from(factor_component(&self.storage, axis, class));
            prefix += cycles as i128 * total * block as i128
                + before * block as i128
                + tail as i128 * value;
        }
        usize::try_from(prefix).map_err(|_| ArrayError::InvalidExtent {
            index: items,
            consumed: usize::MAX,
            available: 0,
        })
    }

    #[inline(always)]
    fn shape_span(
        &self,
        requested: usize,
        available: usize,
    ) -> Result<Option<core::ops::Range<usize>>, ArrayError<RecursiveError>> {
        let period = self.meta[0] as usize;
        let cycles = requested / period;
        let tail = requested % period;
        let mut start =
            cycles
                .checked_mul(self.meta[1] as usize)
                .ok_or(ArrayError::InvalidExtent {
                    index: requested,
                    consumed: usize::MAX,
                    available,
                })?;
        for ordinal in 0..tail {
            let class = self.storage[SHAPE_CODES + ordinal] as usize;
            start = start
                .checked_add(get_u16(&self.storage, SHAPE_WIDTHS + class * 2) as usize)
                .ok_or(ArrayError::InvalidExtent {
                    index: requested,
                    consumed: usize::MAX,
                    available,
                })?;
        }
        let class = self.storage[SHAPE_CODES + tail] as usize;
        let width = get_u16(&self.storage, SHAPE_WIDTHS + class * 2) as usize;
        direct_range(Some(start), width, requested, available)
    }

    #[inline(always)]
    fn periodic_span(
        &self,
        requested: usize,
        available: usize,
    ) -> Result<Option<core::ops::Range<usize>>, ArrayError<RecursiveError>> {
        let period = self.meta[0] as usize;
        let run_len = self.meta[1] as usize;
        let run = requested / run_len;
        let class = run % period;
        let cycles = run / period;
        let mut prefix = 0usize;
        for ordinal in 0..class {
            prefix = prefix
                .checked_add(get_u32(&self.storage, ordinal * 4) as usize)
                .ok_or(ArrayError::InvalidExtent {
                    index: requested,
                    consumed: usize::MAX,
                    available,
                })?;
        }
        let width = get_u32(&self.storage, class * 4) as usize;
        let start = cycles
            .checked_mul(self.meta[2] as usize)
            .and_then(|value| value.checked_mul(run_len))
            .and_then(|value| value.checked_add(prefix * run_len))
            .and_then(|value| value.checked_add(requested % run_len * width));
        direct_range(start, width, requested, available)
    }

    #[inline(always)]
    fn packed_span(
        &self,
        requested: usize,
        available: usize,
    ) -> Result<Option<core::ops::Range<usize>>, ArrayError<RecursiveError>> {
        let run_len = self.meta[1] as usize;
        let run = requested / run_len;
        let block = run / PACKED_RUN_BLOCK;
        let first_run = block * PACKED_RUN_BLOCK;
        let mut start = get_u32(&self.storage, PACKED_PREFIXES + block * 4) as usize;
        for ordinal in first_run..run {
            let class = get_code(&self.storage, PACKED_CODES, ordinal);
            let width = get_u16(&self.storage, PACKED_PALETTE + class * 2) as usize;
            start = start
                .checked_add(
                    width
                        .checked_mul(run_len)
                        .ok_or(ArrayError::InvalidExtent {
                            index: requested,
                            consumed: usize::MAX,
                            available,
                        })?,
                )
                .ok_or(ArrayError::InvalidExtent {
                    index: requested,
                    consumed: usize::MAX,
                    available,
                })?;
        }
        let class = get_code(&self.storage, PACKED_CODES, run);
        let width = get_u16(&self.storage, PACKED_PALETTE + class * 2) as usize;
        start = start
            .checked_add((requested % run_len).checked_mul(width).ok_or(
                ArrayError::InvalidExtent {
                    index: requested,
                    consumed: usize::MAX,
                    available,
                },
            )?)
            .ok_or(ArrayError::InvalidExtent {
                index: requested,
                consumed: usize::MAX,
                available,
            })?;
        direct_range(Some(start), width, requested, available)
    }
}
#[inline(always)]
fn replay_span<C, Slot, const DEPTH: usize>(
    input: &[u8],
    offset: usize,
    depth: RecursiveDepth,
    requested: usize,
) -> Result<Option<core::ops::Range<usize>>, ArrayError<RecursiveError>>
where
    C: RecursiveFrame<Slot>,
{
    let mut cursor = 0usize;
    for index in 0..=requested {
        let absolute = offset
            .checked_add(cursor)
            .ok_or(ArrayError::InvalidExtent {
                index,
                consumed: usize::MAX,
                available: input.len().saturating_sub(cursor),
            })?;
        let measure = C::skip::<DEPTH>(&input[cursor..], absolute, depth).map_err(|source| {
            ArrayError::Item {
                index,
                source: source.flatten_recursive(absolute),
            }
        })?;
        if measure.consumed == 0 {
            return Err(ArrayError::NonProgress {
                index,
                offset: absolute,
            });
        }
        let end = cursor
            .checked_add(measure.consumed)
            .ok_or(ArrayError::InvalidExtent {
                index,
                consumed: measure.consumed,
                available: input.len().saturating_sub(cursor),
            })?;
        if end > input.len() {
            return Err(ArrayError::InvalidExtent {
                index,
                consumed: measure.consumed,
                available: input.len().saturating_sub(cursor),
            });
        }
        if index == requested {
            return Ok(Some(cursor..end));
        }
        cursor = end;
    }
    unreachable!("requested index is bounded")
}

#[inline(always)]
fn direct_range(
    start: Option<usize>,
    width: usize,
    index: usize,
    available: usize,
) -> Result<Option<core::ops::Range<usize>>, ArrayError<RecursiveError>> {
    let Some(start) = start else {
        return Err(ArrayError::InvalidExtent {
            index,
            consumed: usize::MAX,
            available,
        });
    };
    let end = start.checked_add(width).ok_or(ArrayError::InvalidExtent {
        index,
        consumed: width,
        available: available.saturating_sub(start),
    })?;
    if end > available {
        return Err(ArrayError::InvalidExtent {
            index,
            consumed: width,
            available: available.saturating_sub(start),
        });
    }
    Ok(Some(start..end))
}

#[inline(always)]
fn direct_signed_range(
    start: i128,
    width: Option<i128>,
    index: usize,
    available: usize,
) -> Result<Option<core::ops::Range<usize>>, ArrayError<RecursiveError>> {
    let (Ok(start), Some(Ok(width))) = (usize::try_from(start), width.map(usize::try_from)) else {
        return Err(ArrayError::InvalidExtent {
            index,
            consumed: usize::MAX,
            available,
        });
    };
    direct_range(Some(start), width, index, available)
}
