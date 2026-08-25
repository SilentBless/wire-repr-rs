use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::formula::{FormulaEngine, FormulaError};
use crate::workload::{Case, Level};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FormulaValue {
    pub name: String,
    pub value: f64,
    pub unit: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Finding {
    pub name: String,
    pub level: Level,
    pub message: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Evaluation {
    pub formulas: Vec<FormulaValue>,
    pub findings: Vec<Finding>,
}

impl Evaluation {
    #[must_use]
    pub fn failed(&self) -> bool {
        self.findings
            .iter()
            .any(|finding| finding.level == Level::Error)
    }
}

#[derive(Debug, Error)]
pub enum PolicyError {
    #[error("case `{case}` formula `{formula}` failed: {source}")]
    Formula {
        case: String,
        formula: String,
        #[source]
        source: Box<FormulaError>,
    },
    #[error("case `{case}` rule `{rule}` failed to evaluate: {source}")]
    Rule {
        case: String,
        rule: String,
        #[source]
        source: Box<FormulaError>,
    },
}

pub fn evaluate(
    case: &Case,
    metrics: &BTreeMap<String, f64>,
    engine: &FormulaEngine,
) -> Result<Evaluation, PolicyError> {
    let formulas = case
        .formulas
        .iter()
        .map(|formula| {
            Ok(FormulaValue {
                name: formula.name.clone(),
                value: engine
                    .number(&formula.expression, metrics)
                    .map_err(|source| PolicyError::Formula {
                        case: case.name.clone(),
                        formula: formula.name.clone(),
                        source: Box::new(source),
                    })?,
                unit: formula.unit.clone(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut findings = Vec::new();
    for rule in &case.rules {
        let passes =
            engine
                .boolean(&rule.assertion, metrics)
                .map_err(|source| PolicyError::Rule {
                    case: case.name.clone(),
                    rule: rule.name.clone(),
                    source: Box::new(source),
                })?;
        if !passes {
            findings.push(Finding {
                name: rule.name.clone(),
                level: rule.level,
                message: if rule.message.is_empty() {
                    format!("assertion `{}` is false", rule.assertion)
                } else {
                    rule.message.clone()
                },
            });
        }
    }
    Ok(Evaluation { formulas, findings })
}
