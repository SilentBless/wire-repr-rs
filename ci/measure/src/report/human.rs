use std::fmt::Write;

use crate::workload::Level;

use super::Report;

#[must_use]
pub fn render_human(report: &Report, verbose: bool) -> String {
    let mut output = String::new();
    let _ = writeln!(
        output,
        "wire-repr measure · {} · {}",
        report.compiler, report.target
    );
    for workload in &report.workloads {
        for case in &workload.cases {
            let status = if !case.equivalent
                || case
                    .findings
                    .iter()
                    .any(|finding| finding.level == Level::Error)
            {
                "FAIL"
            } else if case
                .findings
                .iter()
                .any(|finding| finding.level == Level::Attention)
            {
                "ATTENTION"
            } else {
                "PASS"
            };
            let _ = writeln!(output, "{status} {}/{}", workload.name, case.name);
            for finding in &case.findings {
                let level = match finding.level {
                    Level::Error => "error",
                    Level::Attention => "attention",
                    Level::Info => "info",
                };
                let _ = writeln!(output, "  {level}: {} — {}", finding.name, finding.message);
            }
            if verbose {
                for (role, result) in &case.roles {
                    let artifact = result.artifact.as_ref().map_or_else(
                        || "artifact=-".to_owned(),
                        |metrics| {
                            format!(
                                "text={}B insn={} br={} calls={}/{}t/{}i stack={}",
                                metrics.text_bytes,
                                metrics.instructions,
                                metrics.branches,
                                metrics.direct_calls,
                                metrics.tail_calls,
                                metrics.indirect_calls,
                                metrics
                                    .stack_bytes
                                    .map_or_else(|| "-".to_owned(), |value| value.to_string())
                            )
                        },
                    );
                    let runtime = result.runtime.as_ref().map_or_else(
                        || "runtime=-".to_owned(),
                        |summary| {
                            format!(
                                "median={:.2}ns p95={:.2}ns mad={:.2}ns",
                                summary.median_ns, summary.p95_ns, summary.mad_ns
                            )
                        },
                    );
                    let custom = if result.custom.is_empty() {
                        String::new()
                    } else {
                        format!(
                            " custom=[{}]",
                            result
                                .custom
                                .iter()
                                .map(|(name, value)| format!("{name}={value:.3}"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    };
                    let _ = writeln!(output, "  {role}: {artifact} {runtime}{custom}");
                }
                for formula in &case.formulas {
                    let _ = writeln!(
                        output,
                        "  {}={:.3}{}",
                        formula.name, formula.value, formula.unit
                    );
                }
            }
        }
    }
    let _ = writeln!(
        output,
        "summary: {} error · {} attention · {} info",
        report.summary.errors, report.summary.attentions, report.summary.information
    );
    output
}
