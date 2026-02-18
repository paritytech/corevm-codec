use crate::{quake, Args};
use anyhow::anyhow;
use corevm_codec::video;
use std::{
    io::{BufRead, BufReader, Read, Write},
    time::{Duration, Instant},
};

pub fn from_quake(
    Args {
        width,
        height,
        max_frames,
        quantization_level,
        verbose,
        ..
    }: Args,
) -> anyhow::Result<()> {
    let mut t_corevm = Duration::ZERO;
    let mut config = video::Config::default();
    config.quantization_level = quantization_level;
    let mut encoder = video::Encoder::new(width, height, config);
    {
        let mut buf = Vec::new();
        encoder.start(&mut buf);
        std::io::stdout().write_all(&buf)?;
    }
    let frame_len = usize::from(width.get()) * usize::from(height.get());
    let mut reader = BufReader::new(std::io::stdin());
    let mut num_frames: u32 = 0;
    let mut num_bytes_read: u64 = 0;
    let mut total_frames: u32 = 0;
    let mut last_reported = Instant::now();
    let mut last_total_frames: u32 = 0;
    let mut palette = [0_u8; 256 * 3];
    let mut has_palette = [0; 1];
    let mut frame = vec![0_u8; frame_len];
    let mut found_palette = false;
    let mut overall_stats = video::Stats::default();
    loop {
        if Some(total_frames) == max_frames.map(|x| x.get()) {
            break;
        }
        let old_num_bytes_read = num_bytes_read;
        match reader.read_exact(&mut has_palette[..]) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
        num_bytes_read += 1;
        let has_palette = (has_palette[0] & 1) != 0;
        if has_palette {
            reader.read_exact(&mut palette[..])?;
            num_bytes_read += palette.len() as u64;
            found_palette = true;
        }
        if !has_palette && !found_palette {
            return Err(anyhow!("Palette not found in the first frame!"));
        }
        reader.read_exact(&mut frame[..])?;
        num_bytes_read += frame.len() as u64;
        let rgb = quake::rgb888_indexed8_to_rgb888(&palette, &frame[..]);
        let mut block = Vec::new();
        let t = Instant::now();
        let stats = encoder.write_rgb888_frame(&rgb, &mut block);
        t_corevm += t.elapsed();
        std::io::stdout().write_all(&block)?;
        overall_stats += &stats;
        num_frames += 1;
        total_frames += 1;
        let now = Instant::now();
        let dt = now.duration_since(last_reported);
        if dt > Duration::from_secs(1) {
            let fps = (total_frames - last_total_frames) as f64 / dt.as_secs_f64();
            log::info!("Processed {total_frames} frames, {fps:.2} frame(s)/s");
            last_reported = now;
            last_total_frames = total_frames;
        }
        if verbose {
            let original_len = num_bytes_read - old_num_bytes_read;
            let compressed_len = block.len();
            eprintln!(
                "Frame {num_frames}: compressed size {:.2} KiB / {:.2} KiB ({:.1}% of the original)",
                compressed_len as f64 * 1e-3,
                original_len as f64 * 1e-3,
                compressed_len as f64 / original_len as f64 * 100.0,
            );
            eprintln!("\n# Frame {num_frames}");
            eprint!(
                "{}",
                DisplayStats {
                    stats: &stats,
                    num_bytes_read: original_len
                }
            );
        }
    }
    if num_frames != 0 {
        let mut block = Vec::new();
        let t = Instant::now();
        encoder.finish(&mut block);
        t_corevm += t.elapsed();
        eprintln!("\n# Total");
        eprint!(
            "{}",
            DisplayStats {
                stats: &overall_stats,
                num_bytes_read
            }
        );
        eprintln!("Encoding time: {:.9}", t_corevm.as_secs_f64());
    }
    Ok(())
}

pub fn to_rgb888(Args { max_frames, .. }: Args) -> anyhow::Result<()> {
    let mut reader = jam_codec::IoReader(BufReader::new(std::io::stdin()));
    let mut decoder = video::Decoder::new(&mut reader)?;
    let frame_len = usize::from(decoder.width().get()) * usize::from(decoder.height().get() * 3);
    let mut frame = vec![0_u8; frame_len];
    let mut num_frames = 0;
    while !reader.0.fill_buf()?.is_empty() {
        if Some(num_frames) == max_frames.map(|x| x.get()) {
            break;
        }
        decoder.read_rgb888_frame(&mut reader, &mut frame)?;
        std::io::stdout().write_all(&frame)?;
        num_frames += 1;
    }
    Ok(())
}

struct DisplayStats<'a> {
    stats: &'a video::Stats,
    num_bytes_read: u64,
}

impl core::fmt::Display for DisplayStats<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Compression ratio.
        let mut rans_streams = [
            ("Y", &self.stats.y),
            ("U", &self.stats.u),
            ("V", &self.stats.v),
        ];
        rans_streams.sort_unstable_by_key(|(_name, stats, ..)| stats.total_out());
        rans_streams.reverse();
        let total_size: u64 = rans_streams
            .iter()
            .map(|(_name, stats, ..)| stats.total_out())
            .sum();
        writeln!(f, "\n## Space")?;
        writeln!(
            f,
            "{:10}  {:>12} {:>11}  {:>10}",
            "Stream", "Size", "Percentage", "Freq"
        )?;
        for (name, stats) in rans_streams.iter() {
            let output_size = stats.total_out();
            let output_size_kib = output_size as f64 * 1e-3;
            let percentage = output_size as f64 / total_size as f64 * 100.0;
            writeln!(
                f,
                "{name:10}  {output_size_kib:>8.2} KiB {percentage:>10.1}%  {:>10.1}",
                stats.num_freqs as f64 / stats.count as f64,
            )?;
        }
        // Timing.
        let mut time = [
            ("Transform", self.stats.time.transform),
            ("Delta", self.stats.time.delta),
            ("rANS", self.stats.time.signmag),
        ];
        time.sort_unstable_by_key(|(_name, duration)| *duration);
        time.reverse();
        let total_duration: Duration = time.iter().map(|(_name, duration)| *duration).sum();
        let total_duration_secs = total_duration.as_secs_f64();
        writeln!(f, "\n## Time")?;
        writeln!(f, "{:10}  {:>11} {:>11}", "Stage", "Duration", "Percentage")?;
        for (name, duration) in time.iter() {
            let secs = duration.as_secs_f64();
            let percentage = secs / total_duration_secs * 100.0;
            writeln!(
                f,
                "{name:10}  {:>11} {percentage:>10.1}%",
                format_duration(*duration)
            )?;
        }
        writeln!(
            f,
            "{:10}  {:>11} {:>10.1}%",
            "Total",
            format_duration(total_duration),
            100.0
        )?;
        writeln!(f, "\n## Compressed size")?;
        let compressed_len = total_size;
        let original_len = self.num_bytes_read;
        writeln!(
            f,
            "{:.2} KiB / {:.2} KiB ({:.1}% of the original)",
            compressed_len as f64 * 1e-3,
            original_len as f64 * 1e-3,
            compressed_len as f64 / original_len as f64 * 100.0,
        )?;
        Ok(())
    }
}

fn format_duration(d: Duration) -> String {
    let nanos = d.as_nanos();
    let (secs, prefix) = match nanos {
        0 => (0.0, ""),
        _ if nanos < 1_000 => (nanos as f64, "n"),
        _ if nanos < 1_000_000 => (nanos as f64 * 1e-3, "μ"),
        _ if nanos < 1_000_000_000 => (nanos as f64 * 1e-6, "m"),
        _ => (nanos as f64 * 1e-9, ""),
    };
    format!("{secs:.1} {prefix}s")
}
