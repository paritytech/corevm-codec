use anyhow::anyhow;
use clap::{Parser, ValueEnum};
use std::{num::NonZero, process::ExitCode};

mod corevm;
mod quake;

#[derive(Parser, Debug)]
#[clap(about = "Transcode video streams between CoreVM and raw formats")]
pub struct Args {
    /// Input format.
    #[clap(short = 'f', long = "input-format")]
    input_format: InputFormat,

    /// Output format.
    #[clap(short = 'F', long = "output-format")]
    output_format: OutputFormat,

    /// Frame width.
    #[clap(short = 'W', long = "width")]
    width: NonZero<u16>,

    /// Frame height.
    #[clap(short = 'H', long = "height")]
    height: NonZero<u16>,

    /// Max. number of frames to transcode.
    #[clap(long = "max-frames")]
    max_frames: Option<NonZero<u32>>,

    /// Quantization level.
    #[clap(short = 'q', long = "quantization-level", default_value_t = 4)]
    quantization_level: u8,

    /// Print per-frame statistics.
    #[clap(action, short = 'v', long = "verbose")]
    verbose: bool,
}

#[derive(ValueEnum, Debug, Default, Clone, Copy, PartialEq, Eq)]
enum InputFormat {
    /// CoreVM video stream.
    #[default]
    Corevm,
    /// Raw Quake frames (indexed RGB888).
    Quake,
}

#[derive(ValueEnum, Debug, Default, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    /// CoreVM video stream.
    #[default]
    Corevm,
    /// Raw RGB888 frames.
    Rgb888,
}

fn main() -> ExitCode {
    match do_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn do_main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = Args::parse();
    type I = InputFormat;
    type O = OutputFormat;
    match (args.input_format, args.output_format) {
        (I::Quake, O::Corevm) => corevm::from_quake(args),
        (I::Quake, O::Rgb888) => quake::to_rgb888(args),
        (I::Corevm, O::Rgb888) => corevm::to_rgb888(args),
        (I::Corevm, O::Corevm) => Err(anyhow!("Can't transcode to the same format")),
    }
}
