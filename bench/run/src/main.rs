use std::{ffi::CString, path::PathBuf, process::ExitCode, time::Instant};

use anyhow::anyhow;
use clap::Parser;
use corevm_host::Outcome;
use jam_types::{max_exports, SignedGas};

mod engine;

use self::engine::*;

#[derive(Parser)]
#[clap(about, arg_required_else_help = true)]
struct Args {
    /// Max. inner VM gas.
    #[clap(long = "gas", value_name = "NUM", default_value_t = SignedGas::MAX)]
    inner_vm_gas: SignedGas,

    /// Max. no. of memory pages an inner VM can allocate.
    #[clap(long = "max-pages", value_name = "NUM")]
    max_pages: Option<u16>,

    /// How many times to resume the program when it runs out of gas or memory
    /// pages.
    #[clap(long = "max-steps", value_name = "NUM", default_value_t = u64::MAX)]
    max_steps: u64,

    /// Path to the directory that will be used as the root of the virtual file
    /// system.
    #[clap(short = 'r', long = "rootfs", value_name = "DIR")]
    rootfs_dir: Option<PathBuf>,

    /// Environment variables.
    #[clap(short = 'e', long = "env", value_name = "VAR=VALUE")]
    env: Vec<CString>,

    /// Zeroth argument.
    #[clap(short = 'a', long = "arg0", value_name = "ARG0", default_value = "app")]
    arg0: CString,

    /// Benchmark performance.
    #[clap(action, long = "perf")]
    perf: bool,

    /// Path to an executable compiled for Polka VM.
    #[clap(value_name = "EXE")]
    exe_path: PathBuf,

    /// Executable arguments.
    #[clap(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "ARGS"
    )]
    args: Vec<CString>,
}

fn main() -> ExitCode {
    do_main().unwrap_or_else(|e| {
        eprintln!("{e}");
        ExitCode::FAILURE
    })
}

fn do_main() -> anyhow::Result<ExitCode> {
    env_logger::try_init()?;
    let cli_args = Args::parse();
    let exe_args = std::iter::once(cli_args.arg0)
        .chain(cli_args.args)
        .map(corevm_host::Arg)
        .collect();
    let exe_env = cli_args.env.into_iter().map(corevm_host::Arg).collect();
    let program_file = cli_args.exe_path;
    let root_dir = cli_args.rootfs_dir;
    let num_steps = cli_args.max_steps;
    let program_blob = std::fs::read(&program_file)
        .map_err(|e| anyhow!("Failed to read {program_file:?}: {e}"))?;
    let inner_vm_gas = SignedGas::MAX;
    let mut engine = LocalEngine::new(
        inner_vm_gas,
        &program_blob[..],
        root_dir.as_deref(),
        exe_args,
        exe_env,
        max_exports() as u16,
    )?;
    let t = Instant::now();
    for step in 1..=num_steps {
        let (work_output, outer_vm) = engine.step()?;
        let outcome = work_output.vm_output.outcome;
        match outcome {
            Outcome::TimeLimitReached
            | Outcome::OutputLimitReached
            | Outcome::PageFault { .. }
            | Outcome::OutOfGas => {
                engine.finalize_step(inner_vm_gas, &work_output, outer_vm)?;
            }
            Outcome::Halt => break,
            Outcome::Panic => return Err(anyhow!("Panicked at step {step}")),
        }
    }
    if cli_args.perf {
        eprintln!("Execution time: {:.9} s", t.elapsed().as_secs_f64());
    }
    Ok(ExitCode::SUCCESS)
}
