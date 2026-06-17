//! GIF compression handler — palette optimization and frame dropping.

use crate::core::CompressionSettings;
use gif::{DisposalMethod, Encoder, Frame, Repeat};
use std::borrow::Cow;
use std::fs;
use std::path::Path;

/// Compress a GIF file by reducing colors and optionally dropping frames.
pub fn compress_gif(path: &Path, settings: &CompressionSettings) -> Result<(), String> {
    let data = fs::read(path).map_err(|e| format!("Failed to read GIF: {}", e))?;

    let mut decode_opts = gif::DecodeOptions::new();
    decode_opts.set_color_output(gif::ColorOutput::Indexed);

    let mut decoder = decode_opts
        .read_info(std::io::Cursor::new(&data))
        .map_err(|e| format!("Failed to decode GIF: {}", e))?;

    let width = decoder.width();
    let height = decoder.height();

    // Read global color table (palette)
    let global_palette = decoder.global_palette().map(|p| p.to_vec());

    let mut frames: Vec<Frame<'static>> = Vec::new();
    while let Some(frame_result) = decoder.read_next_frame().transpose() {
        let frame_ref = frame_result.map_err(|e| format!("Failed to read frame: {}", e))?;
        // Convert borrowed frame to owned frame (Cow::Owned)
        frames.push(to_owned_frame(frame_ref));
    }

    if frames.is_empty() {
        return Err("GIF has no frames".into());
    }

    // Frame dropping: keep every Nth frame based on target fps
    let target_fps = settings.gif_target_fps.max(1) as usize;
    let frames = if settings.gif_frame_drop {
        // Estimate original fps from average delay (delay is in centiseconds)
        let avg_delay: f64 = frames
            .iter()
            .map(|f| f.delay as f64)
            .sum::<f64>()
            / frames.len() as f64;
        let original_fps = if avg_delay > 0.0 {
            100.0 / avg_delay
        } else {
            15.0
        };

        if original_fps > target_fps as f64 {
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

    // Recalculate delay for new frame rate
    let new_delay = (100.0 / target_fps as f64) as u16;

    // Build a palette: use the global palette if available, otherwise quantize
    let palette = if settings.gif_palette_optimize {
        build_optimized_palette(&frames, width, height, &global_palette)
    } else {
        global_palette.unwrap_or_default()
    };

    let num_colors = palette.len() / 3;

    // Encode the optimized GIF
    let tmp_path = path.with_extension("tmp.gif");
    let tmp_file = fs::File::create(&tmp_path).map_err(|e| format!("Failed to create temp GIF: {}", e))?;

    let mut encoder = Encoder::new(tmp_file, width, height, &palette[..num_colors * 3])
        .map_err(|e| format!("Failed to create GIF encoder: {}", e))?;
    encoder
        .set_repeat(Repeat::Infinite)
        .map_err(|e| format!("Failed to set repeat: {}", e))?;

    for mut frame in frames {
        // Re-quantize pixels to the new palette if palette was rebuilt
        if settings.gif_palette_optimize && num_colors > 0 {
            let pixels = requantize_frame(&frame, &palette[..num_colors * 3]);
            frame.buffer = Cow::Owned(pixels);
        }
        frame.delay = new_delay;
        frame.dispose = DisposalMethod::Any;
        encoder
            .write_frame(&frame)
            .map_err(|e| format!("Failed to write frame: {}", e))?;
    }

    drop(encoder); // flush

    fs::rename(&tmp_path, path).map_err(|e| format!("Failed to replace original GIF: {}", e))?;

    Ok(())
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

/// Build an optimized (reduced) color palette from the frames.
/// This collects all colors across frames, counts frequency, and keeps the
/// most common 256 (or fewer) colors.
fn build_optimized_palette(
    frames: &[Frame<'static>],
    width: u16,
    height: u16,
    global_palette: &Option<Vec<u8>>,
) -> Vec<u8> {
    use std::collections::HashMap;

    // Count color frequency across all frames
    let mut color_counts: HashMap<(u8, u8, u8), u64> = HashMap::new();

    for frame in frames {
        let pal = frame
            .palette
            .as_deref()
            .or(global_palette.as_deref());

        if let Some(palette) = pal {
            // For indexed frames, count palette colors weighted by usage
            let mut usage = vec![0u64; palette.len() / 3];
            for &idx in frame.buffer.iter() {
                if (idx as usize) < usage.len() {
                    usage[idx as usize] += 1;
                }
            }
            for (i, &count) in usage.iter().enumerate() {
                if count > 0 && i * 3 + 2 < palette.len() {
                    let r = palette[i * 3];
                    let g = palette[i * 3 + 1];
                    let b = palette[i * 3 + 2];
                    *color_counts.entry((r, g, b)).or_default() += count;
                }
            }
        }
    }

    // Sort by frequency, keep top colors (up to 256)
    let mut colors: Vec<((u8, u8, u8), u64)> = color_counts.into_iter().collect();
    colors.sort_by(|a, b| b.1.cmp(&a.1));

    let max_colors = 256usize;
    let mut palette = Vec::with_capacity(max_colors * 3);

    // Always include black and white as first two entries
    palette.extend_from_slice(&[0, 0, 0]);
    palette.extend_from_slice(&[255, 255, 255]);

    for &(color, _) in colors.iter().take(max_colors - 2) {
        if !palette
            .chunks(3)
            .any(|c| c[0] == color.0 && c[1] == color.1 && c[2] == color.2)
        {
            palette.push(color.0);
            palette.push(color.1);
            palette.push(color.2);
        }
    }

    // Pad to a power of 2 (GIF requires palette size to be power of 2)
    let num_colors = palette.len() / 3;
    let target = num_colors.next_power_of_two().min(256);
    while palette.len() / 3 < target {
        palette.push(0);
        palette.push(0);
        palette.push(0);
    }

    let _ = (width, height); // suppress unused warnings
    palette
}

/// Re-quantize a frame's indexed pixels to the nearest color in the new palette.
fn requantize_frame(frame: &Frame<'static>, new_palette: &[u8]) -> Vec<u8> {
    let old_palette = frame
        .palette
        .as_deref()
        .unwrap_or(&[]);

    if old_palette.is_empty() {
        // No old palette, assume indices map directly
        return frame.buffer.to_vec();
    }

    // Build mapping: old_index -> new_index
    let old_colors = old_palette.len() / 3;
    let mut mapping = vec![0u8; old_colors];

    for old_idx in 0..old_colors {
        let r = old_palette[old_idx * 3];
        let g = old_palette[old_idx * 3 + 1];
        let b = old_palette[old_idx * 3 + 2];

        // Find closest color in new palette
        let new_idx = find_closest_color(r, g, b, new_palette);
        mapping[old_idx] = new_idx;
    }

    frame.buffer.iter().map(|&idx| {
        if (idx as usize) < mapping.len() {
            mapping[idx as usize]
        } else {
            0
        }
    }).collect()
}

/// Find the index of the closest color in a palette using Euclidean distance.
fn find_closest_color(r: u8, g: u8, b: u8, palette: &[u8]) -> u8 {
    let mut best_idx = 0u8;
    let mut best_dist = u32::MAX;

    let num_colors = palette.len() / 3;
    for i in 0..num_colors {
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
