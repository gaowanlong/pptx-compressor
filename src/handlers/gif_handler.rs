//! GIF compression handler — palette optimization, frame dropping and metadata fidelity.
//!
//! The rebuilt palette must be applied to *every* frame using the same source
//! palette that was used to count colors (local table first, then the global
//! table). Mixing the two is what previously produced badly discolored GIFs.
//!
//! Two pieces of animation metadata are now carried over verbatim:
//! - the Netscape loop count (previously everything was forced to infinite), and
//! - the transparent index (previously re-mapped to "the closest color", which
//!   could collide with an opaque color and punch holes in solid areas).

use crate::core::CompressionSettings;
use crate::handlers::CompressCandidate;
use gif::{Encoder, Frame};
use std::borrow::Cow;
use std::fs;
use std::io::Cursor;
use std::path::Path;

/// Compress a GIF into `candidate`. Local frame tables are folded into one
/// global table when palette optimization is enabled.
pub fn compress_gif(
    path: &Path,
    candidate: &Path,
    settings: &CompressionSettings,
) -> Result<CompressCandidate, String> {
    let data = fs::read(path).map_err(|e| format!("读取 GIF 失败：{}", e))?;

    let mut decode_opts = gif::DecodeOptions::new();
    decode_opts.set_color_output(gif::ColorOutput::Indexed);

    let mut decoder = decode_opts
        .read_info(Cursor::new(&data))
        .map_err(|e| format!("解码 GIF 失败：{}", e))?;

    let width = decoder.width();
    let height = decoder.height();
    let global_palette: Option<Vec<u8>> = decoder.global_palette().map(|p| p.to_vec());
    // read_info already consumed the Netscape application extension. A GIF
    // without it returns Finite(0), which the encoder writes as "no extension"
    // (play once) instead of silently turning it into an infinite loop.
    let repeat = decoder.repeat();

    let mut frames: Vec<Frame<'static>> = Vec::new();
    while let Some(frame_result) = decoder.read_next_frame().transpose() {
        let frame_ref = frame_result.map_err(|e| format!("读取 GIF 帧失败：{}", e))?;
        frames.push(to_owned_frame(frame_ref));
    }

    if frames.is_empty() {
        return Err("GIF 中没有可处理的帧".into());
    }
    if global_palette.is_none() && !frames.iter().any(|f| f.palette.is_some()) {
        return Err("GIF 缺少调色板".into());
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
    let has_transparency = frames.iter().any(|f| f.transparent.is_some());
    let mut palette = if optimize {
        build_optimized_palette(&frames, global_palette.as_deref())
    } else {
        global_palette.clone().unwrap_or_default()
    };

    // Rebuilding the palette can point the transparent index at a slot that is
    // also used by opaque pixels. Reserve one slot exclusively for transparency
    // so no opaque pixel can ever be turned transparent by accident.
    let transparent_slot = if optimize && has_transparency {
        Some(reserve_transparent_slot(&mut palette))
    } else {
        None
    };

    let num_colors = palette.len() / 3;
    if num_colors == 0 {
        return Err("GIF has no usable global palette (local-only color tables)".into());
    }

    let tmp_file = fs::File::create(candidate)
        .map_err(|e| format!("创建 GIF 候选文件失败：{}", e))?;
    let mut encoder = Encoder::new(tmp_file, width, height, &palette[..num_colors * 3])
        .map_err(|e| format!("创建 GIF 编码器失败：{}", e))?;
    encoder
        .set_repeat(repeat)
        .map_err(|e| format!("设置 GIF 循环次数失败：{}", e))?;

    for mut frame in frames {
        if optimize {
            frame.buffer = Cow::Owned(requantize_frame(
                &frame,
                &palette,
                global_palette.as_deref(),
                transparent_slot,
            ));
            // All frames now use the rebuilt global table; a stale local table
            // would point the same indices at the wrong colors.
            frame.palette = None;
            if let Some(slot) = transparent_slot {
                frame.transparent = Some(slot);
            }
        }
        if dropped {
            frame.delay = new_delay;
        }
        encoder
            .write_frame(&frame)
            .map_err(|e| format!("写入 GIF 帧失败：{}", e))?;
    }
    drop(encoder); // flush

    Ok(CompressCandidate { path: candidate.to_path_buf(), new_extension: None })
}

/// Reserve one palette slot that opaque pixels will never be mapped to.
/// GIF color tables must be a power of two (2..=256 entries).
fn reserve_transparent_slot(palette: &mut Vec<u8>) -> u8 {
    let count = palette.len() / 3;
    if count >= 256 {
        // Table is full: sacrifice the last entry. Opaque pixels are remapped
        // to their next closest color, transparency stays exact.
        return 255;
    }
    let slot = count as u8;
    palette.extend_from_slice(&[0, 0, 0]);
    let target = (count + 1).next_power_of_two().clamp(2, 256);
    while palette.len() / 3 < target {
        palette.extend_from_slice(&[0, 0, 0]);
    }
    slot
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
/// Transparent pixels are excluded: their palette entry is arbitrary and would
/// only steal a slot from colors that are actually visible.
fn build_optimized_palette(frames: &[Frame<'static>], global_palette: Option<&[u8]>) -> Vec<u8> {
    use std::collections::HashMap;

    let mut color_counts: HashMap<(u8, u8, u8), u64> = HashMap::new();
    for frame in frames {
        let pal = frame.palette.as_deref().or(global_palette);
        if let Some(palette) = pal {
            let mut usage = vec![0u64; palette.len() / 3];
            for &idx in frame.buffer.iter() {
                if frame.transparent == Some(idx) {
                    continue;
                }
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
    colors.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    // An all-transparent frame must still yield a usable (2-entry) table.
    if colors.is_empty() {
        colors.push(((0, 0, 0), 1));
    }

    let mut palette = Vec::with_capacity(256 * 3);
    for &(color, _) in colors.iter().take(256) {
        palette.extend_from_slice(&[color.0, color.1, color.2]);
    }

    // GIF palettes must be a power of two (and at least two entries).
    let num_colors = palette.len() / 3;
    let target = num_colors.next_power_of_two().clamp(2, 256);
    while palette.len() / 3 < target {
        palette.extend_from_slice(&[0, 0, 0]);
    }
    palette
}

/// Re-quantize indexed pixels from the frame's source palette to the new one.
/// Transparent pixels are sent to the dedicated slot instead of being matched
/// to a color, and opaque pixels never land in that slot.
fn requantize_frame(
    frame: &Frame<'static>,
    new_palette: &[u8],
    global_palette: Option<&[u8]>,
    transparent_slot: Option<u8>,
) -> Vec<u8> {
    let old_palette = frame.palette.as_deref().or(global_palette).unwrap_or(&[]);
    if old_palette.is_empty() {
        // Without a source palette colors cannot be remapped, but transparency
        // still has to point at the dedicated slot.
        return frame
            .buffer
            .iter()
            .map(|&idx| match (frame.transparent, transparent_slot) {
                (Some(t), Some(slot)) if idx == t => slot,
                _ => idx,
            })
            .collect();
    }

    let old_colors = old_palette.len() / 3;
    let mut mapping = vec![0u8; old_colors];
    for old_idx in 0..old_colors {
        mapping[old_idx] = find_closest_color(
            old_palette[old_idx * 3],
            old_palette[old_idx * 3 + 1],
            old_palette[old_idx * 3 + 2],
            new_palette,
            transparent_slot,
        );
    }
    frame
        .buffer
        .iter()
        .map(|&idx| match (frame.transparent, transparent_slot) {
            (Some(t), Some(slot)) if idx == t => slot,
            _ => mapping.get(idx as usize).copied().unwrap_or(0),
        })
        .collect()
}

/// Find the index of the closest color in a palette using Euclidean distance.
/// `exclude` keeps the reserved transparent slot out of the candidate set.
fn find_closest_color(r: u8, g: u8, b: u8, palette: &[u8], exclude: Option<u8>) -> u8 {
    let mut best_idx: Option<u8> = None;
    let mut best_dist = u32::MAX;
    for i in 0..palette.len() / 3 {
        let idx = i as u8;
        if exclude == Some(idx) {
            continue;
        }
        let dr = r as i32 - palette[i * 3] as i32;
        let dg = g as i32 - palette[i * 3 + 1] as i32;
        let db = b as i32 - palette[i * 3 + 2] as i32;
        let dist = (dr * dr + dg * dg + db * db) as u32;
        if dist < best_dist {
            best_dist = dist;
            best_idx = Some(idx);
        }
    }
    // A table has at least two entries and at most one is excluded, so this
    // fallback is unreachable in practice; it only guards a malformed palette.
    best_idx.unwrap_or(0)
}
