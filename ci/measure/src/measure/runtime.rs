use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Summary {
    pub samples: usize,
    pub median_ns: f64,
    pub p95_ns: f64,
    pub minimum_ns: f64,
    pub maximum_ns: f64,
    pub mad_ns: f64,
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("runtime sampling produced no values")]
    Empty,
    #[error("runtime sample is not finite: {0}")]
    NonFinite(f64),
}

pub fn summarize(samples: &[f64]) -> Result<Summary, RuntimeError> {
    if samples.is_empty() {
        return Err(RuntimeError::Empty);
    }
    if let Some(value) = samples.iter().copied().find(|value| !value.is_finite()) {
        return Err(RuntimeError::NonFinite(value));
    }
    let mut ordered = samples.to_vec();
    ordered.sort_by(f64::total_cmp);
    let median_ns = median(&ordered);
    let p95_index = ((ordered.len() as f64 * 0.95).ceil() as usize)
        .saturating_sub(1)
        .min(ordered.len() - 1);
    let mut deviations = ordered
        .iter()
        .map(|value| (value - median_ns).abs())
        .collect::<Vec<_>>();
    deviations.sort_by(f64::total_cmp);
    Ok(Summary {
        samples: ordered.len(),
        median_ns,
        p95_ns: ordered[p95_index],
        minimum_ns: ordered[0],
        maximum_ns: ordered[ordered.len() - 1],
        mad_ns: median(&deviations),
    })
}

pub fn interleaved_roles(roles: &[String], sample: usize) -> Vec<String> {
    if roles.is_empty() {
        return Vec::new();
    }
    let offset = sample % roles.len();
    roles[offset..]
        .iter()
        .chain(&roles[..offset])
        .cloned()
        .collect()
}

pub fn calibration_next(current: u64, elapsed_ns: u128, target_ms: u64) -> u64 {
    if elapsed_ns == 0 {
        return current.saturating_mul(100).max(100);
    }
    let target_ns = u128::from(target_ms) * 1_000_000;
    let scaled = u128::from(current)
        .saturating_mul(target_ns)
        .checked_div(elapsed_ns)
        .unwrap_or(u128::from(u64::MAX))
        .clamp(1, u128::from(u64::MAX));
    u64::try_from(scaled).unwrap_or(u64::MAX)
}

fn median(ordered: &[f64]) -> f64 {
    let middle = ordered.len() / 2;
    if ordered.len().is_multiple_of(2) {
        (ordered[middle - 1] + ordered[middle]) / 2.0
    } else {
        ordered[middle]
    }
}
