//! GIF compression handler — palette optimization and frame dropping.
//!
//! The rebuilt palette must be applied to *every* frame using the same source
//! palette that was used to count colors (local table first, then the global
//! table). Mixing the two is what previously produced badly discolored GIFs.

use crate::core::CompressionSettings;
use crate::handlers::CompressCandidate;
use gif::{Encoder, Frame, Repeat};
use std::borrow::Cow;
use std::fs;
use std::path::Path;

/// Compress a GIF into `candidate`. Local frame tables are folded into one
/// global table when palette optimization is enabled.
pub fn compress_gif(
    path: &Path,
    candidate: &Path,
    settings: &CompressionSettings,
) -> Result<CompressCandidate, String> {
    let data = fs::read(path).map_err(|e| format!("Failed to read GIF: {}", e))?;

    let mut decode_opts = gif::DecodeOptions::new();
    decode_opts.set_color_output(gif::ColorOutput::Indexed);

    let mut decoder = decode_opts
        .read_info(std::io::Cursor::new(&data))
        .map_err(|e| format!("Failed to decode GIF: {}", e))?;

    let width = decoder.width();
    let height = decoder.height();
    let global_palette: Option<Vec<u8>> = decoder.global_palette().map(|p| p.to_vec());

    let mut frames: Vec<Frame<'static>> = Vec::new();
    while let Some(frame_result) = decoder.read_next_frame().transpose() {
        let frame_ref = frame_result.map_err(|e| format!("Failed to read frame: {}", e))?;
        frames.push(to_owned_frame(&frame_ref));
    }

    if frames.is_empty() {
        return Err("GIF has no frames".into());
    }
    if global_palette.is_none() && !frames.iter().any(|f| f.palette.is_some()) {
        return Err("GIF has no color table".into());
    }

    // Frame dropping: keep every Nth frame based on target fps. Delay is only
    // rewritten when frames were actually removed.
    let target_fps = settings.gif_target_fps.max(1) as usize;
    let mut dropped = false;
    let frames = if settings.gif_frame_drop {
        let avg_delay: f64 =
            frames.iter().map(|f| f.delay as f64).sum::<f64>() / frames.len() as f64;
        let original_fps = if avg_delay > 0.0 { 100.0 / avg_delay } else { 15.0 };
        if original_fps > target_fps as f64 {
            dropped = true;
            let keep_every = (original_fps / target_fps as f64).ceil() as usize;
            frames
                .into_iter()
                .enumerate()
                .filter(|(i, _)| i % keep_every == 0)
                .map(|(_, f)| f)
                .collect()
        } else {
            frames
        }
    } else {
        frames
    };
    let new_delay = (100.0 / target_fps as f64).max(1.0) as u16;

    let optimize = settings.gif_palette_optimize;
    let palette = if optimize {
        build_optimized_palette(&frames, global_palette.as_deref())
    } else {
        global_palette.clone().unwrap_or_default()
    };
    let num_colors = palette.len() / 3;
    if num_colors == 0 {
        return Err("GIF has no usable global palette (local-only color tables)".into());
    }

    let tmp_file = fs::File::create(candidate)
        .map_err(|e| format!("Failed to create GIF candidate: {}", e))?;
    let mut encoder = Encoder::new(tmp_file, width, height, &palette[..num_colors * 3])
        .map_err(|e| format!("Failed to create GIF encoder: {}", e))?;
    encoder
        .set_repeat(Repeat::Infinite)
        .map_err(|e| format!("Failed to set repeat: {}", e))?;

    for mut frame in frames {
        if optimize {
            let source_palette = frame
                .palette
                .as_deref()
                .or(global_palette.as_deref())
                .unwrap_or(&[]);
            if let Some(t) = frame.transparent {
                frame.transparent = Some(remap_index(t, source_palette, &palette));
            }
            frame.buffer = Cow::Owned(requantize_frame(
                &frame,
                &palette,
                global_palette.as_deref(),
            ));
            // All frames now use the rebuilt global table; a stale local table
            // would point the same indices at the wrong colors.
            frame.palette = None;
        }
        if dropped {
            frame.delay = new_delay;
        }
        encoder
            .write_frame(&frame)
            .map_err(|e| format!("Failed to write frame: {}", e))?;
    }
    drop(encoder); // flush

    Ok(CompressCandidate { path: candidate.to_path_buf(), new_extension: None })
}

/// Convert a borrowed Frame into an owned Frame with 'static lifetime.
fn to_owned_frame(frame_ref: &Frame) -> Frame<'static> {
    Frame {
        delay: frame_ref.delay,
        dispose: frame_ref.dispose,
        transparent: frame_ref.transparent,
        needs_user_input: frame_ref.needs_user_input,
        top: frame_ref.top,
        left: frame_ref.left,
        buffer: Cow::Owned(frame_ref.buffer.to_vec()),
        width: frame_ref.width,
        height: frame_ref.height,
        interlaced: frame_ref.interlaced,
        palette: frame_ref.palette.clone(),
    }
}

/// Build a compact palette from the most frequently used colors across frames.
fn build_optimized_palette(frames: &[Frame<'static>], global_palette: Option<&[u8]>) -> Vec<u8> {
    use std::collections::HashMap;

    let mut color_counts: HashMap<(u8, u8, u8), u64> = HashMap::new();
    for frame in frames {
        let pal = frame.palette.as_deref().or(global_palette);
        if let Some(palette) = pal {
            let mut usage = vec![0u64; palette.len() / 3];
            for &idx in frame.buffer.iter() {
                if (idx as usize) < usage.len() {
                    usage[idx as usize] += 1;
                }
            }
            for (i, &count) in usage.iter().enumerate() {
                if count > 0 && i * 3 + 2 < palette.len() {
                    let color = (palette[i * 3], palette[i * 3 + 1], palette[i * 3 + 2]);
                    *color_counts.entry(color).or_default() += count;
                }
            }
        }
    }

    let mut colors: Vec<((u8, u8, u8), u64)> = color_counts.into_iter().collect();
    colors.sort_by(|a, b| b.1.cmp(&a.1));

    let mut palette = Vec::with_capacity(256 * 3);
    for &(color, _) in colors.iter().take(256) {
        palette.extend_from_slice(&[color.0, color.1, color.2]);
    }

    // GIF palettes must be a power of two (and at least two entries).
    let num_colors = palette.len() / 3;
    let target = num_colors.next_power_of_two().max(2).min(256);
    while palette.len() / 3 < target {
        palette.extend_from_slice(&[0, 0, 0]);
    }
    palette
}

/// Re-quantize indexed pixels from the frame's source palette to the new one.
fn requantize_frame(
    frame: &Frame<'static>,
    new_palette: &[u8],
    global_palette: Option<&[u8]>,
) -> Vec<u8> {
    let old_palette = frame.palette.as_deref().or(global_palette).unwrap_or(&[]);
    if old_palette.is_empty() {
        return frame.buffer.to_vec();
    }
    let old_colors = old_palette.len() / 3;
    let mut mapping = vec![0u8; old_colors];
    for old_idx in 0..old_colors {
        mapping[old_idx] = find_closest_color(
            old_palette[old_idx * 3],
            old_palette[old_idx * 3 + 1],
            old_palette[old_idx * 3 + 2],
            new_palette,
        );
    }
    frame
        .buffer
        .iter()
        .map(|&idx| mapping.get(idx as usize).copied().unwrap_or(0))
        .collect()
}

fn remap_index(index: u8, old_palette: &[u8], new_palette: &[u8]) -> u8 {
    let i = index as usize;
    if i * 3 + 2 >= old_palette.len() {
        return 0;
    }
    find_closest_color(
        old_palette[i * 3],
        old_palette[i * 3 + 1],
        old_palette[i * 3 + 2],
        new_palette,
    )
}

/// Find the index of the closest color in a palette using Euclidean distance.
fn find_closest_color(r: u8, g: u8, b: u8, palette: &[u8]) -> u8 {
    let mut best_idx = 0u8;
    let mut best_dist = u32::MAX;
    for i in 0..palette.len() / 3 {
        let dr = r as i32 - palette[i * 3] as i32;
        let dg = g as i32 - palette[i * 3 + 1] as i32;
        let db = b as i32 - palette[i * 3 + 2] as i32;
        let dist = (dr * dr + dg * dg + db * db) as u32;
        if dist < best_dist {
            best_dist = dist;
            best_idx = i as u8;
        }
    }
    best_idx
}
