use std::collections::BTreeMap;

use evalexpr::{
    ContextWithMutableFunctions, ContextWithMutableVariables, DefaultNumericTypes, EvalexprError,
    Function, HashMapContext, Value, build_operator_tree,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FormulaError {
    #[error("invalid formula environment: {0}")]
    Environment(#[from] EvalexprError),
    #[error("formula `{expression}` failed: {source}")]
    Evaluation {
        expression: String,
        source: EvalexprError,
    },
}

#[derive(Clone, Debug, Default)]
pub struct FormulaEngine;

impl FormulaEngine {
    pub fn new() -> Result<Self, FormulaError> {
        let _ = base_context()?;
        Ok(Self)
    }

    pub fn number(
        &self,
        expression: &str,
        metrics: &BTreeMap<String, f64>,
    ) -> Result<f64, FormulaError> {
        let context = context(metrics)?;
        let tree = build_operator_tree::<DefaultNumericTypes>(expression).map_err(|source| {
            FormulaError::Evaluation {
                expression: expression.to_owned(),
                source,
            }
        })?;
        tree.eval_number_with_context(&context)
            .map_err(|source| FormulaError::Evaluation {
                expression: expression.to_owned(),
                source,
            })
    }

    pub fn boolean(
        &self,
        expression: &str,
        metrics: &BTreeMap<String, f64>,
    ) -> Result<bool, FormulaError> {
        let context = context(metrics)?;
        let tree = build_operator_tree::<DefaultNumericTypes>(expression).map_err(|source| {
            FormulaError::Evaluation {
                expression: expression.to_owned(),
                source,
            }
        })?;
        tree.eval_boolean_with_context(&context)
            .map_err(|source| FormulaError::Evaluation {
                expression: expression.to_owned(),
                source,
            })
    }
}

fn context(
    metrics: &BTreeMap<String, f64>,
) -> Result<HashMapContext<DefaultNumericTypes>, FormulaError> {
    let mut context = base_context()?;
    for (name, value) in metrics {
        let value =
            if value.fract() == 0.0 && *value >= i64::MIN as f64 && *value <= i64::MAX as f64 {
                Value::Int(*value as i64)
            } else {
                Value::Float(*value)
            };
        context.set_value(name.clone(), value)?;
    }
    Ok(context)
}

fn base_context() -> Result<HashMapContext<DefaultNumericTypes>, FormulaError> {
    let mut context = HashMapContext::<DefaultNumericTypes>::new();
    context.set_function(
        "ratio".to_owned(),
        Function::new(|argument| binary(argument, |left, right| left / right)),
    )?;
    context.set_function(
        "delta".to_owned(),
        Function::new(|argument| binary(argument, |left, right| left - right)),
    )?;
    context.set_function(
        "percent".to_owned(),
        Function::new(|argument| {
            binary(argument, |left, right| {
                if right == 0.0 && left == 0.0 {
                    0.0
                } else {
                    (left - right) / right * 100.0
                }
            })
        }),
    )?;
    Ok(context)
}

fn binary(
    argument: &Value<DefaultNumericTypes>,
    operation: impl FnOnce(f64, f64) -> f64,
) -> Result<Value<DefaultNumericTypes>, EvalexprError> {
    let values = argument.as_tuple()?;
    if values.len() != 2 {
        return Err(EvalexprError::wrong_function_argument_amount(
            values.len(),
            2,
        ));
    }
    Ok(Value::Float(operation(
        values[0].as_number()?,
        values[1].as_number()?,
    )))
}
