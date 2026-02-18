use crate::Args;
use anyhow::anyhow;
use std::io::{BufReader, Read, Write};

pub fn rgb888_indexed8_to_rgb888(palette: &[u8], indices: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(indices.len() * 3);
    for i in indices.iter().copied() {
        let j = 3 * i as usize;
        rgb.extend_from_slice(&palette[j..j + 3]);
    }
    rgb
}

pub fn to_rgb888(
    Args {
        width,
        height,
        max_frames,
        ..
    }: Args,
) -> anyhow::Result<()> {
    let frame_len = usize::from(width.get()) * usize::from(height.get());
    let mut reader = BufReader::new(std::io::stdin());
    let mut palette = [0_u8; 256 * 3];
    let mut has_palette = [0; 1];
    let mut frame = vec![0_u8; frame_len];
    let mut found_palette = false;
    let mut num_frames = 0;
    loop {
        if Some(num_frames) == max_frames.map(|x| x.get()) {
            break;
        }
        num_frames += 1;
        match reader.read_exact(&mut has_palette[..]) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
        let has_palette = (has_palette[0] & 1) != 0;
        if has_palette {
            reader.read_exact(&mut palette[..])?;
            found_palette = true;
        }
        if !has_palette && !found_palette {
            return Err(anyhow!("Palette not found in the first frame!"));
        }
        reader.read_exact(&mut frame[..])?;
        let rgb = rgb888_indexed8_to_rgb888(&palette, &frame[..]);
        std::io::stdout().write_all(&rgb)?;
    }
    Ok(())
}
