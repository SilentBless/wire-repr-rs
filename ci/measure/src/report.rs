use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::measure::artifact::Metrics as ArtifactMetrics;
use crate::measure::runtime::Summary as RuntimeSummary;
use crate::policy::{Finding, FormulaValue};

mod human;
mod json;

pub use human::render_human;
pub use json::{JsonError, render_json};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Report {
    pub compiler: String,
    pub target: String,
    pub workloads: Vec<WorkloadReport>,
    pub summary: RunSummary,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkloadReport {
    pub name: String,
    pub cases: Vec<CaseReport>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CaseReport {
    pub name: String,
    pub equivalent: bool,
    pub roles: BTreeMap<String, RoleReport>,
    pub formulas: Vec<FormulaValue>,
    pub findings: Vec<Finding>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct RoleReport {
    pub artifact: Option<ArtifactMetrics>,
    pub runtime: Option<RuntimeSummary>,
    pub custom: BTreeMap<String, f64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct RunSummary {
    pub errors: usize,
    pub attentions: usize,
    pub information: usize,
}
