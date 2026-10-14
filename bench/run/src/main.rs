use anyhow::{Context, anyhow};
use clap::{Parser, Subcommand};
use polkavm::{
    BackendKind, CacheModel, Config, CostModelKind, Engine, GasMeteringKind, InterruptKind, Module,
    ModuleConfig, ProgramBlob, RawInstance,
};
use polkavm_linker::{TargetInstructionSet, TargetJsonArgs};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const GAS_LIMIT: i64 = i64::MAX;
const MIN_STACK_SIZE: u32 = 1024 * 1024;

#[derive(Parser)]
#[clap(about)]
struct Args {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print the path of the PolkaVM target specification for building the
    /// guest program.
    TargetJsonPath,
    /// Run the benchmarks under PolkaVM.
    Run {
        /// Guest program ELF file.
        guest: PathBuf,
        /// Raw RGB888 frames.
        frames: PathBuf,
        #[clap(long, default_value_t = 320)]
        width: u32,
        #[clap(long, default_value_t = 200)]
        height: u32,
        #[clap(long, default_value_t = 4)]
        quantization_level: u32,
    },
}

struct Measurement {
    result: u32,
    gas: i64,
    time: Duration,
}

fn call(instance: &mut RawInstance, function: &str, args: &[u64]) -> anyhow::Result<Measurement> {
    let program_counter = instance
        .module()
        .exports()
        .find(|export| export.symbol().as_bytes() == function.as_bytes())
        .ok_or_else(|| anyhow!("missing export '{function}'"))?
        .program_counter();
    instance.set_gas(GAS_LIMIT);
    instance.prepare_call_untyped(program_counter, args);
    let start = Instant::now();
    let interrupt = instance
        .run()
        .with_context(|| format!("failed to run '{function}'"))?;
    let time = start.elapsed();
    if !matches!(interrupt, InterruptKind::Finished) {
        return Err(anyhow!("'{function}' interrupted: {interrupt:?}"));
    }
    Ok(Measurement {
        result: instance.get_result_typed::<u32>(),
        gas: GAS_LIMIT - instance.gas(),
        time,
    })
}

fn instantiate(guest: &Path) -> anyhow::Result<RawInstance> {
    let elf = std::fs::read(guest).with_context(|| format!("failed to read {guest:?}"))?;
    let mut linker_config = polkavm_linker::Config::default();
    linker_config.set_strip(true);
    linker_config.set_min_stack_size(MIN_STACK_SIZE);
    let blob = polkavm_linker::program_from_elf(linker_config, TargetInstructionSet::JamV1, &elf)
        .with_context(|| format!("failed to link {guest:?}"))?;
    let blob = ProgramBlob::parse(blob.into()).context("failed to parse program blob")?;
    let mut config = Config::from_env()?;
    config.set_backend(Some(BackendKind::Compiler));
    let engine = Engine::new(&config)?;
    let mut module_config = ModuleConfig::new();
    module_config.set_gas_metering(Some(GasMeteringKind::Sync));
    module_config.set_cost_model(Some(CostModelKind::Full(CacheModel::L2Hit)));
    let module = Module::from_blob(&engine, &module_config, blob)?;
    Ok(module.instantiate()?)
}

fn print_measurement(name: &str, measurement: &Measurement, frame_count: usize) {
    println!(
        "  {name}: {:.0} ms, {:.1} M gas per frame",
        measurement.time.as_secs_f64() * 1000.0,
        measurement.gas as f64 / frame_count as f64 / 1_000_000.0
    );
}

fn print_size(encoded_len: u32, raw_len: usize) {
    println!(
        "  Compressed size: {} KiB, compression ratio {:.1}%",
        encoded_len / 1024,
        100.0 * (1.0 - f64::from(encoded_len) / raw_len as f64)
    );
}

fn run(
    guest: PathBuf,
    frames: PathBuf,
    width: u32,
    height: u32,
    quantization_level: u32,
) -> anyhow::Result<()> {
    let frames = std::fs::read(&frames).with_context(|| format!("failed to read {frames:?}"))?;
    let frame_count = frames.len() / (width * height * 3) as usize;
    let mut instance = instantiate(&guest)?;
    let frames_address = call(&mut instance, "allocate_frames", &[frames.len() as u64])?.result;
    instance.write_memory(frames_address, &frames)?;
    println!(
        "Frames: {frame_count} ({width}x{height}), raw size {} KiB",
        frames.len() / 1024
    );
    println!("CoreVM (quantization level {quantization_level})");
    let encoding = call(
        &mut instance,
        "encode_corevm",
        &[width.into(), height.into(), quantization_level.into()],
    )?;
    print_size(encoding.result, frames.len());
    print_measurement("Encoding", &encoding, frame_count);
    let decoding = call(&mut instance, "decode_corevm", &[])?;
    print_measurement("Decoding", &decoding, frame_count);
    println!("QOI+rANS");
    let encoding = call(
        &mut instance,
        "encode_qoi_rans",
        &[width.into(), height.into()],
    )?;
    print_size(encoding.result, frames.len());
    print_measurement("Encoding", &encoding, frame_count);
    Ok(())
}

fn main() -> anyhow::Result<()> {
    match Args::parse().command {
        Command::TargetJsonPath => {
            let path = polkavm_linker::target_json_path(TargetJsonArgs::default())
                .map_err(|error| anyhow!("failed to get target JSON path: {error}"))?;
            println!("{}", path.display());
        }
        Command::Run {
            guest,
            frames,
            width,
            height,
            quantization_level,
        } => run(guest, frames, width, height, quantization_level)?,
    }
    Ok(())
}
