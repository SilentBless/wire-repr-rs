use std::collections::BTreeSet;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use wire_repr_measure::engine::{Options, run};
use wire_repr_measure::report::{render_human, render_json};
use wire_repr_measure::workload::discover;

#[derive(Debug, Parser)]
#[command(name = "wire-repr-measure")]
#[command(about = "Measure generated wire-repr code against independent implementations")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run(Run),
    List(List),
}

#[derive(Clone, Debug, Args)]
struct Common {
    #[arg(long, default_value = ".")]
    root: PathBuf,
    #[arg(long, default_value = "wire-repr/measure")]
    workloads: PathBuf,
}

#[derive(Debug, Args)]
struct List {
    #[command(flatten)]
    common: Common,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct Run {
    #[command(flatten)]
    common: Common,
    #[arg(long, default_value = "target/wire-repr-measure")]
    target: PathBuf,
    #[arg(long, default_value = "1.91.0")]
    toolchain: String,
    #[arg(long)]
    filter: Option<String>,
    #[arg(long)]
    workload: Option<String>,
    #[arg(long)]
    no_runtime: bool,
    #[arg(long)]
    json: bool,
    #[arg(long, short)]
    verbose: bool,
}

fn main() -> ExitCode {
    match execute(Cli::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("wire-repr-measure: {error}");
            ExitCode::from(2)
        }
    }
}

fn execute(cli: Cli) -> Result<ExitCode, Box<dyn std::error::Error>> {
    match cli.command {
        Command::List(arguments) => {
            let root = arguments.common.root.canonicalize()?;
            let workloads = discover(&root.join(arguments.common.workloads))?;
            let names = workloads
                .iter()
                .map(|workload| workload.config.name.as_str())
                .collect::<Vec<_>>();
            if names.is_empty()
                || names.iter().copied().collect::<BTreeSet<_>>().len() != names.len()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "workload inventory must be non-empty and unique",
                )
                .into());
            }
            if arguments.json {
                println!("{}", serde_json::to_string(&names)?);
            } else {
                for name in names {
                    println!("{name}");
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Run(arguments) => {
            let root = arguments.common.root.canonicalize()?;
            let report = run(&Options {
                workspace: root.clone(),
                workloads: root.join(arguments.common.workloads),
                target: root.join(arguments.target),
                toolchain: arguments.toolchain,
                filter: arguments.filter,
                workload: arguments.workload,
                runtime: !arguments.no_runtime,
            })?;
            if arguments.json {
                println!("{}", render_json(&report)?);
            } else {
                print!("{}", render_human(&report, arguments.verbose));
            }
            Ok(if report.summary.errors == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
    }
}
