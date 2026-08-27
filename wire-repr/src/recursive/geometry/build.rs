use super::storage::*;
use super::{RecursiveGeometry, RecursiveMeasure};
use crate::ArrayError;
use crate::recursive::{FlattenRecursiveError, RecursiveDepth, RecursiveError, RecursiveFrame};

// moved geometry construction
/// In-place bounded geometry summary used while framing one array.
#[doc(hidden)]
pub struct RecursiveGeometryBuilder {
    period_widths: [u16; PERIOD_CAPACITY],
    palette: [u16; PALETTE_CAPACITY],
    candidates: u64,
    run_count: usize,
    current_width: usize,
    current_len: u32,
    common_run_len: u32,
    palette_len: usize,
    fixed_width: Option<usize>,
    affine_base: u32,
    affine_slope: i32,
    items: usize,
    started: bool,
    periodic_failed: bool,
    common_runs_failed: bool,
    palette_failed: bool,
    affine_failed: bool,
    saw_nested: bool,
    compressible: bool,
    ranked_candidate: bool,
    factor_candidate: bool,
    factor_valid: bool,
    factor_high_component: bool,
    factor_base: u32,
    last_palette_width: u16,
    last_palette_class: u8,
    has_last_palette: bool,
    shape_period: u8,
    shape_classes: u8,
    shape_attempted: bool,
    shape_candidate: bool,
}

impl RecursiveGeometryBuilder {
    /// Creates an empty geometry accumulator.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            period_widths: [0; PERIOD_CAPACITY],
            palette: [0; PALETTE_CAPACITY],
            candidates: u64::MAX,
            run_count: 0,
            current_width: 0,
            current_len: 0,
            common_run_len: 0,
            palette_len: 0,
            fixed_width: None,
            affine_base: 0,
            affine_slope: 0,
            items: 0,
            started: false,
            periodic_failed: false,
            common_runs_failed: false,
            palette_failed: false,
            affine_failed: false,
            saw_nested: false,
            compressible: true,
            ranked_candidate: false,
            factor_candidate: false,
            factor_valid: false,
            factor_high_component: false,
            factor_base: 0,
            last_palette_width: 0,
            last_palette_class: 0,
            has_last_palette: false,
            shape_period: 0,
            shape_classes: 0,
            shape_attempted: false,
            shape_candidate: false,
        }
    }

    fn prepare(&mut self, count: usize, geometry: &mut RecursiveGeometry) {
        self.ranked_candidate = count <= RANKED_CAPACITY;
        self.factor_candidate = count > RANKED_CAPACITY;
        self.factor_valid = self.factor_candidate;
        if self.ranked_candidate {
            geometry.reset(GEOMETRY_RANKED);
        } else {
            geometry.reset(GEOMETRY_FACTORIZED);
        }
    }

    /// Observes one exact generated item measurement.
    pub fn push(
        &mut self,
        index: usize,
        start: usize,
        measure: RecursiveMeasure,
        geometry: &mut RecursiveGeometry,
    ) {
        let width = measure.consumed;
        self.saw_nested |= measure.nested_depth != 0;
        let Ok(width32) = u32::try_from(width) else {
            self.compressible = false;
            self.items += 1;
            return;
        };
        self.fixed_width = match self.fixed_width {
            None => Some(width),
            Some(previous) if previous == width => Some(previous),
            Some(_) => Some(usize::MAX),
        };
        if self.items == 0 {
            self.affine_base = width32;
        } else if self.items == 1 {
            let slope = i64::from(width32) - i64::from(self.affine_base);
            match i32::try_from(slope) {
                Ok(slope) => self.affine_slope = slope,
                Err(_) => self.affine_failed = true,
            }
        } else if !self.affine_failed {
            let expected =
                i128::from(self.affine_base) + i128::from(self.affine_slope) * self.items as i128;
            self.affine_failed = expected != i128::from(width32);
        }

        let mut palette_class = None;
        if !self.palette_failed {
            if let Ok(width16) = u16::try_from(width) {
                if self.has_last_palette && self.last_palette_width == width16 {
                    palette_class = Some(usize::from(self.last_palette_class));
                } else {
                    palette_class = self.palette[..self.palette_len]
                        .iter()
                        .position(|candidate| *candidate == width16);
                    if palette_class.is_none() {
                        if self.palette_len == PALETTE_CAPACITY {
                            self.palette_failed = true;
                        } else {
                            palette_class = Some(self.palette_len);
                            self.palette[self.palette_len] = width16;
                            self.palette_len += 1;
                        }
                    }
                    if let Some(class) = palette_class {
                        self.last_palette_width = width16;
                        self.last_palette_class = class as u8;
                        self.has_last_palette = true;
                    }
                }
                if self.ranked_candidate
                    && let Some(class) = palette_class
                {
                    put_u16(&mut geometry.storage, RANKED_PALETTE + class * 2, width16);
                    set_code(&mut geometry.storage, RANKED_CODES, index, class);
                    if index.is_multiple_of(RANKED_BLOCK) {
                        if let Ok(start) = u32::try_from(start) {
                            put_u32(
                                &mut geometry.storage,
                                RANKED_PREFIXES + index / RANKED_BLOCK * 4,
                                start,
                            );
                        } else {
                            self.ranked_candidate = false;
                        }
                    }
                }
            } else {
                self.palette_failed = true;
            }
        }
        self.ranked_candidate &= !self.palette_failed && palette_class.is_some();

        if self.shape_candidate
            || (index == 0 && measure.nested_depth != 0 && self.factor_candidate)
        {
            if !self.shape_attempted {
                geometry.reset(GEOMETRY_RECURSIVE_SHAPE);
                self.shape_attempted = true;
                self.shape_candidate = true;
                self.factor_candidate = false;
                self.periodic_failed = true;
                self.factor_valid = false;
            }
            self.push_shape(index, width, measure.shape, geometry);
        }
        if self.factor_candidate && self.factor_valid {
            self.push_factor(index, width32, geometry);
        }

        if !self.started {
            self.started = true;
            self.current_width = width;
            self.current_len = 1;
        } else if self.current_width == width {
            self.current_len = self.current_len.saturating_add(1);
        } else {
            self.finish_run(false);
            self.current_width = width;
            self.current_len = 1;
        }
        self.items += 1;
    }

    fn push_factor(&mut self, index: usize, width: u32, geometry: &mut RecursiveGeometry) {
        if index == 0 {
            self.factor_base = width;
            for axis in 0..FACTOR_LENGTHS.len() {
                set_factor_component(&mut geometry.storage, axis, 0, 0);
            }
        }
        let digits = factor_digits(index);
        for axis in 0..FACTOR_LENGTHS.len() {
            if factor_initialized(&geometry.storage, axis, digits[axis]) {
                continue;
            }
            let mut known = i64::from(self.factor_base);
            for (other, digit) in digits.iter().copied().enumerate() {
                if other != axis {
                    known += i64::from(factor_component(&geometry.storage, other, digit));
                }
            }
            let Ok(delta) = i16::try_from(i64::from(width) - known) else {
                self.factor_valid = false;
                return;
            };
            set_factor_component(&mut geometry.storage, axis, digits[axis], delta);
            self.factor_high_component |= axis > 0 && delta != 0;
        }
        let predicted = i64::from(self.factor_base)
            + (0..FACTOR_LENGTHS.len())
                .map(|axis| i64::from(factor_component(&geometry.storage, axis, digits[axis])))
                .sum::<i64>();
        self.factor_valid &= predicted == i64::from(width);
    }

    fn push_shape(
        &mut self,
        index: usize,
        width: usize,
        shape: u64,
        geometry: &mut RecursiveGeometry,
    ) {
        if !self.shape_candidate {
            return;
        }
        let (Ok(width), shape) = (u16::try_from(width), shape as u16) else {
            self.shape_candidate = false;
            return;
        };
        let mut class = None;
        for candidate in 0..usize::from(self.shape_classes) {
            if get_u16(&geometry.storage, SHAPE_HASHES + candidate * 2) == shape {
                if get_u16(&geometry.storage, SHAPE_WIDTHS + candidate * 2) != width {
                    self.shape_candidate = false;
                }
                class = Some(candidate);
                break;
            }
        }
        let class = match class {
            Some(class) => class,
            None if usize::from(self.shape_classes) < PALETTE_CAPACITY => {
                let class = usize::from(self.shape_classes);
                put_u16(&mut geometry.storage, SHAPE_HASHES + class * 2, shape);
                put_u16(&mut geometry.storage, SHAPE_WIDTHS + class * 2, width);
                self.shape_classes += 1;
                class
            }
            None => {
                self.shape_candidate = false;
                return;
            }
        };
        let code = class as u8;
        if index < PERIOD_CAPACITY {
            geometry.storage[SHAPE_CODES + index] = code;
            let mut prefix = if index == 0 {
                0
            } else {
                usize::from(self.period_widths[index - 1])
            };
            while prefix != 0 && code != geometry.storage[SHAPE_CODES + prefix] {
                prefix = usize::from(self.period_widths[prefix - 1]);
            }
            if index != 0 && code == geometry.storage[SHAPE_CODES + prefix] {
                prefix += 1;
            }
            self.period_widths[index] = prefix as u16;
            if index + 1 == PERIOD_CAPACITY {
                self.shape_period = (PERIOD_CAPACITY - prefix) as u8;
            }
        } else {
            let period = usize::from(self.shape_period);
            self.shape_candidate = code == geometry.storage[SHAPE_CODES + index % period];
        }
    }

    fn complete(&mut self) {
        if self.started {
            self.finish_run(true);
            self.started = false;
        }
    }

    fn finish_run(&mut self, final_run: bool) {
        let Ok(width) = u16::try_from(self.current_width) else {
            self.periodic_failed = true;
            self.common_runs_failed = true;
            self.run_count += 1;
            return;
        };
        if self.common_run_len == 0 {
            self.common_run_len = self.current_len;
        } else if (!final_run && self.current_len != self.common_run_len)
            || (final_run && self.current_len > self.common_run_len)
        {
            self.periodic_failed = true;
            self.common_runs_failed = true;
        }
        let index = self.run_count;
        if !self.shape_attempted && index < PERIOD_CAPACITY {
            self.period_widths[index] = width;
        }
        if !self.periodic_failed {
            for period in 1..=index.min(PERIOD_CAPACITY) {
                if width != self.period_widths[index % period] {
                    self.candidates &= !(1u64 << (period - 1));
                }
            }
            self.periodic_failed = self.candidates == 0;
        }
        self.run_count += 1;
    }
}

impl Default for RecursiveGeometryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Frames one counted recursive array and selects the strongest proven bounded geometry.
#[doc(hidden)]
pub fn frame_recursive_array_extent<C, Slot, const DEPTH: usize>(
    input: &[u8],
    count: usize,
    offset: usize,
    depth: RecursiveDepth,
    geometry: &mut RecursiveGeometry,
) -> Result<usize, ArrayError<RecursiveError>>
where
    C: RecursiveFrame<Slot>,
{
    let mut builder = RecursiveGeometryBuilder::new();
    builder.prepare(count, geometry);
    let consumed =
        measure_all::<C, Slot, DEPTH>(input, count, offset, depth, |index, start, measure| {
            builder.push(index, start, measure, geometry);
            Ok(())
        })?;
    builder.complete();
    choose_geometry::<C, Slot, DEPTH>(
        &input[..consumed],
        count,
        offset,
        depth,
        &builder,
        geometry,
    )?;
    Ok(consumed)
}

fn choose_geometry<C, Slot, const DEPTH: usize>(
    input: &[u8],
    count: usize,
    offset: usize,
    depth: RecursiveDepth,
    builder: &RecursiveGeometryBuilder,
    geometry: &mut RecursiveGeometry,
) -> Result<(), ArrayError<RecursiveError>>
where
    C: RecursiveFrame<Slot>,
{
    if !builder.compressible {
        geometry.reset(GEOMETRY_REPLAY);
        return Ok(());
    }
    if count == 0 {
        geometry.reset(GEOMETRY_FIXED);
        geometry.meta[0] = 0;
        return Ok(());
    }
    if let Some(width) = builder.fixed_width
        && width != usize::MAX
        && let Ok(width) = u32::try_from(width)
    {
        geometry.reset(GEOMETRY_FIXED);
        geometry.meta[0] = width;
        return Ok(());
    }
    if count >= 2 && !builder.affine_failed {
        geometry.reset(GEOMETRY_FORMULA);
        geometry.meta[0] = builder.affine_base;
        geometry.meta[1] = builder.affine_slope as u32;
        return Ok(());
    }
    if builder.shape_candidate && builder.shape_period != 0 {
        let period = usize::from(builder.shape_period);
        let mut cycle = 0u32;
        for index in 0..period {
            let class = usize::from(geometry.storage[SHAPE_CODES + index]);
            cycle = cycle
                .checked_add(u32::from(get_u16(
                    &geometry.storage,
                    SHAPE_WIDTHS + class * 2,
                )))
                .ok_or(ArrayError::InvalidExtent {
                    index,
                    consumed: usize::MAX,
                    available: input.len(),
                })?;
        }
        geometry.meta[0] = period as u32;
        geometry.meta[1] = cycle;
        geometry.meta[2] = u32::from(builder.shape_classes);
        return Ok(());
    }
    if builder.factor_candidate && builder.factor_valid && builder.factor_high_component {
        geometry.meta[0] = builder.factor_base;
        return Ok(());
    }
    if builder.saw_nested && count > RANKED_CAPACITY && !builder.shape_attempted {
        geometry.reset(GEOMETRY_RECURSIVE_SHAPE);
        if build_recursive_shape::<C, Slot, DEPTH>(input, count, offset, depth, geometry)? {
            return Ok(());
        }
    }
    if !builder.periodic_failed
        && let Some(period) = (1..=builder.run_count.min(PERIOD_CAPACITY))
            .find(|period| builder.candidates & (1u64 << (period - 1)) != 0)
    {
        geometry.reset(GEOMETRY_PERIODIC);
        let mut cycle = 0u32;
        for index in 0..period {
            put_u32(
                &mut geometry.storage,
                index * 4,
                u32::from(builder.period_widths[index]),
            );
            cycle = cycle
                .checked_add(u32::from(builder.period_widths[index]))
                .ok_or(ArrayError::InvalidExtent {
                    index,
                    consumed: usize::MAX,
                    available: input.len(),
                })?;
        }
        geometry.meta[0] = period as u32;
        geometry.meta[1] = builder.common_run_len;
        geometry.meta[2] = cycle;
        return Ok(());
    }
    if builder.run_count <= INTERVAL_CAPACITY {
        geometry.reset(GEOMETRY_INTERVAL);
        if build_intervals::<C, Slot, DEPTH>(input, count, offset, depth, geometry)? {
            return Ok(());
        }
    }
    if !builder.factor_candidate && !builder.ranked_candidate {
        geometry.reset(GEOMETRY_FACTORIZED);
        if build_factorized::<C, Slot, DEPTH>(input, count, offset, depth, geometry)? {
            return Ok(());
        }
    }
    if builder.ranked_candidate {
        geometry.meta[0] = builder.palette_len as u32;
        geometry.meta[1] = count as u32;
        return Ok(());
    }
    if !builder.common_runs_failed
        && builder.run_count <= PACKED_RUN_CAPACITY
        && !builder.palette_failed
    {
        geometry.reset(GEOMETRY_PACKED_RUNS);
        if build_packed_runs::<C, Slot, DEPTH>(
            input,
            count,
            offset,
            depth,
            builder.common_run_len,
            geometry,
        )? {
            return Ok(());
        }
    }
    geometry.reset(GEOMETRY_REPLAY);
    Ok(())
}

fn build_recursive_shape<C, Slot, const DEPTH: usize>(
    input: &[u8],
    count: usize,
    offset: usize,
    depth: RecursiveDepth,
    geometry: &mut RecursiveGeometry,
) -> Result<bool, ArrayError<RecursiveError>>
where
    C: RecursiveFrame<Slot>,
{
    let mut classes = 0usize;
    let mut candidates = u64::MAX;
    let mut observed = [0u8; PERIOD_CAPACITY];
    let mut valid = true;
    measure_all::<C, Slot, DEPTH>(input, count, offset, depth, |index, _, measure| {
        if !valid {
            return Ok(());
        }
        let width = match u16::try_from(measure.consumed) {
            Ok(width) => width,
            Err(_) => {
                valid = false;
                return Ok(());
            }
        };
        let shape = measure.shape as u16;
        let mut class = None;
        for candidate in 0..classes {
            if get_u16(&geometry.storage, SHAPE_HASHES + candidate * 2) == shape {
                if get_u16(&geometry.storage, SHAPE_WIDTHS + candidate * 2) != width {
                    valid = false;
                }
                class = Some(candidate);
                break;
            }
        }
        let class = match class {
            Some(class) => class,
            None if classes < PALETTE_CAPACITY => {
                let class = classes;
                put_u16(&mut geometry.storage, SHAPE_HASHES + class * 2, shape);
                put_u16(&mut geometry.storage, SHAPE_WIDTHS + class * 2, width);
                classes += 1;
                class
            }
            None => {
                valid = false;
                return Ok(());
            }
        };
        let code = class as u8;
        if index < PERIOD_CAPACITY {
            observed[index] = code;
        }
        for period in 1..=PERIOD_CAPACITY {
            if index >= period && code != observed[index % period] {
                candidates &= !(1u64 << (period - 1));
            }
        }
        valid &= candidates != 0;
        Ok(())
    })?;
    if !valid {
        return Ok(false);
    }
    let Some(period) =
        (1..=count.min(PERIOD_CAPACITY)).find(|period| candidates & (1u64 << (period - 1)) != 0)
    else {
        return Ok(false);
    };
    let mut cycle = 0u32;
    for (index, code) in observed.iter().copied().enumerate().take(period) {
        geometry.storage[SHAPE_CODES + index] = code;
        cycle = cycle
            .checked_add(u32::from(get_u16(
                &geometry.storage,
                SHAPE_WIDTHS + code as usize * 2,
            )))
            .ok_or(ArrayError::InvalidExtent {
                index,
                consumed: usize::MAX,
                available: input.len(),
            })?;
    }
    geometry.meta[0] = period as u32;
    geometry.meta[1] = cycle;
    geometry.meta[2] = classes as u32;
    Ok(true)
}

fn build_intervals<C, Slot, const DEPTH: usize>(
    input: &[u8],
    count: usize,
    offset: usize,
    depth: RecursiveDepth,
    geometry: &mut RecursiveGeometry,
) -> Result<bool, ArrayError<RecursiveError>>
where
    C: RecursiveFrame<Slot>,
{
    let mut previous = None;
    let mut segments = 0usize;
    let mut valid = true;
    measure_all::<C, Slot, DEPTH>(input, count, offset, depth, |index, start, measure| {
        if !valid || previous == Some(measure.consumed) {
            return Ok(());
        }
        if segments == INTERVAL_CAPACITY {
            valid = false;
            return Ok(());
        }
        let (Ok(index), Ok(start), Ok(width)) = (
            u32::try_from(index),
            u32::try_from(start),
            u32::try_from(measure.consumed),
        ) else {
            valid = false;
            return Ok(());
        };
        let base = segments * 12;
        put_u32(&mut geometry.storage, base, index);
        put_u32(&mut geometry.storage, base + 4, start);
        put_u32(&mut geometry.storage, base + 8, width);
        previous = Some(measure.consumed);
        segments += 1;
        Ok(())
    })?;
    if !valid {
        return Ok(false);
    }
    geometry.meta[0] = segments as u32;
    Ok(true)
}

fn build_packed_runs<C, Slot, const DEPTH: usize>(
    input: &[u8],
    count: usize,
    offset: usize,
    depth: RecursiveDepth,
    run_len: u32,
    geometry: &mut RecursiveGeometry,
) -> Result<bool, ArrayError<RecursiveError>>
where
    C: RecursiveFrame<Slot>,
{
    if run_len == 0 {
        return Ok(false);
    }
    let mut palette = 0usize;
    let mut runs = 0usize;
    let mut current = None;
    let mut current_len = 0u32;
    let mut current_start = 0usize;
    let mut valid = true;
    {
        let mut finish_run = |width: usize,
                              length: u32,
                              start: usize,
                              final_run: bool,
                              geometry: &mut RecursiveGeometry| {
            if !valid || (!final_run && length != run_len) || length > run_len {
                valid = false;
                return;
            }
            if runs == PACKED_RUN_CAPACITY {
                valid = false;
                return;
            }
            let Ok(width) = u16::try_from(width) else {
                valid = false;
                return;
            };
            let mut class = None;
            for candidate in 0..palette {
                if get_u16(&geometry.storage, PACKED_PALETTE + candidate * 2) == width {
                    class = Some(candidate);
                    break;
                }
            }
            let class = match class {
                Some(class) => class,
                None if palette < PALETTE_CAPACITY => {
                    let class = palette;
                    put_u16(&mut geometry.storage, PACKED_PALETTE + class * 2, width);
                    palette += 1;
                    class
                }
                None => {
                    valid = false;
                    return;
                }
            };
            set_code(&mut geometry.storage, PACKED_CODES, runs, class);
            if runs.is_multiple_of(PACKED_RUN_BLOCK) {
                let Ok(start) = u32::try_from(start) else {
                    valid = false;
                    return;
                };
                put_u32(
                    &mut geometry.storage,
                    PACKED_PREFIXES + runs / PACKED_RUN_BLOCK * 4,
                    start,
                );
            }
            runs += 1;
        };
        measure_all::<C, Slot, DEPTH>(input, count, offset, depth, |_, start, measure| {
            match current {
                None => {
                    current = Some(measure.consumed);
                    current_len = 1;
                    current_start = start;
                }
                Some(width) if width == measure.consumed => current_len += 1,
                Some(width) => {
                    finish_run(width, current_len, current_start, false, geometry);
                    current = Some(measure.consumed);
                    current_len = 1;
                    current_start = start;
                }
            }
            Ok(())
        })?;
        if let Some(width) = current {
            finish_run(width, current_len, current_start, true, geometry);
        }
    }
    if !valid {
        return Ok(false);
    }
    geometry.meta[0] = runs as u32;
    geometry.meta[1] = run_len;
    geometry.meta[2] = palette as u32;
    Ok(true)
}
fn build_factorized<C, Slot, const DEPTH: usize>(
    input: &[u8],
    count: usize,
    offset: usize,
    depth: RecursiveDepth,
    geometry: &mut RecursiveGeometry,
) -> Result<bool, ArrayError<RecursiveError>>
where
    C: RecursiveFrame<Slot>,
{
    let mut builder = RecursiveGeometryBuilder::new();
    builder.factor_candidate = true;
    builder.factor_valid = true;
    measure_all::<C, Slot, DEPTH>(input, count, offset, depth, |index, _, measure| {
        let Ok(width) = u32::try_from(measure.consumed) else {
            builder.factor_valid = false;
            return Ok(());
        };
        if builder.factor_valid {
            builder.push_factor(index, width, geometry);
        }
        Ok(())
    })?;
    if !builder.factor_valid || !builder.factor_high_component {
        return Ok(false);
    }
    geometry.meta[0] = builder.factor_base;
    Ok(true)
}

fn measure_all<C, Slot, const DEPTH: usize>(
    input: &[u8],
    count: usize,
    offset: usize,
    depth: RecursiveDepth,
    mut observe: impl FnMut(usize, usize, RecursiveMeasure) -> Result<(), ArrayError<RecursiveError>>,
) -> Result<usize, ArrayError<RecursiveError>>
where
    C: RecursiveFrame<Slot>,
{
    let mut cursor = 0usize;
    for index in 0..count {
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
        observe(index, cursor, measure)?;
        cursor = end;
    }
    Ok(cursor)
}
