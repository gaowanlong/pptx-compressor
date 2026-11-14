//! 元数据保真端到端测试：真实编码器生成带元数据的图片，跑压缩后回读断言。
//! 另含一个完整 PPTX 用例，验证整条批处理流水线不会丢掉媒体元数据。
//! 不依赖 GUI、FFmpeg 或网络。

use gif::{Encoder, Frame, Repeat};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder as _, ImageReader};
use pptx_compressor::core::batch::{run_batch, BatchMessage};
use pptx_compressor::core::CompressionSettings;
use pptx_compressor::handlers::metadata::{
    exif_orientation, inject_png_chunks, jpeg_metadata_segments, png_metadata_chunks,
    PngMetadataChunk,
};
use pptx_compressor::handlers::{gif_handler, image as image_handler};
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};
use tempfile::tempdir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const W: u32 = 24;
const H: u32 = 16;

fn rgb_pixels() -> Vec<u8> {
    image::RgbImage::from_fn(W, H, |x, y| {
        image::Rgb([
            ((x * 9) % 256) as u8,
            ((y * 13) % 256) as u8,
            (((x + y) * 5) % 256) as u8,
        ])
    })
    .into_raw()
}

/// 最小 EXIF TIFF，IFD0 里带一条 Orientation(0x0112) = SHORT 记录。
fn exif_tiff(orientation: u16) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"II");
    out.extend_from_slice(&42u16.to_le_bytes());
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0x0112u16.to_le_bytes());
    out.extend_from_slice(&3u16.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    let mut value = [0u8; 4];
    value[..2].copy_from_slice(&orientation.to_le_bytes());
    out.extend_from_slice(&value);
    out.extend_from_slice(&0u32.to_le_bytes());
    out
}

fn jpeg_with_metadata(orientation: u16, icc: &[u8], quality: u8) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    let mut encoder = JpegEncoder::new_with_quality(&mut buf, quality);
    encoder.set_exif_metadata(exif_tiff(orientation)).unwrap();
    encoder.set_icc_profile(icc.to_vec()).unwrap();
    encoder
        .write_image(&rgb_pixels(), W, H, ExtendedColorType::Rgb8)
        .unwrap();
    buf.into_inner()
}

fn plain_jpeg(quality: u8) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    JpegEncoder::new_with_quality(&mut buf, quality)
        .write_image(&rgb_pixels(), W, H, ExtendedColorType::Rgb8)
        .unwrap();
    buf.into_inner()
}

fn png_with_metadata(chunks: &[PngMetadataChunk]) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    PngEncoder::new(&mut buf)
        .write_image(&rgb_pixels(), W, H, ExtendedColorType::Rgb8)
        .unwrap();
    inject_png_chunks(&buf.into_inner(), chunks).unwrap()
}

fn decode_dimensions(data: &[u8]) -> (u32, u32) {
    let decoded = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .unwrap()
        .decode()
        .unwrap();
    (decoded.width(), decoded.height())
}

fn compress(ext: &str, source: &[u8], settings: &CompressionSettings) -> Vec<u8> {
    let dir = tempdir().unwrap();
    let src = dir.path().join(format!("source.{ext}"));
    let dst = dir.path().join(format!("candidate.{ext}"));
    fs::write(&src, source).unwrap();
    match ext {
        "jpg" => image_handler::compress_jpeg(&src, &dst, settings).unwrap(),
        "png" => image_handler::compress_png(&src, &dst, settings).unwrap(),
        "gif" => gif_handler::compress_gif(&src, &dst, settings).unwrap(),
        _ => panic!("unsupported extension: {ext}"),
    };
    fs::read(&dst).unwrap()
}

fn gif_bytes(repeat: Option<Repeat>, palette: &[u8], pixels: &[u8], transparent: Option<u8>) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut encoder = Encoder::new(&mut buf, 4, 2, palette).unwrap();
        if let Some(value) = repeat {
            encoder.set_repeat(value).unwrap();
        }
        let mut frame = Frame::from_indexed_pixels(4, 2, pixels.to_vec(), transparent);
        frame.delay = 10;
        encoder.write_frame(&frame).unwrap();
    }
    buf
}

fn gif_decoder(data: &[u8]) -> gif::Decoder<Cursor<&[u8]>> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::Indexed);
    options.read_info(Cursor::new(data)).unwrap()
}

fn gif_settings() -> CompressionSettings {
    CompressionSettings {
        gif_frame_drop: false,
        gif_palette_optimize: true,
        ..Default::default()
    }
}

#[test]
fn jpeg_exif_orientation_and_icc_survive_reencode() {
    let icc = b"fake-icc-profile-bytes-0123456789";
    let source = jpeg_with_metadata(6, icc, 95);
    let out = compress("jpg", &source, &CompressionSettings::default());

    let segments = jpeg_metadata_segments(&out);
    let exif = segments.iter().find(|s| s.marker == 0xE1).expect("APP1/EXIF 应保留");
    assert!(exif.payload.starts_with(b"Exif\0\0"));
    // 方向标签原样保留：查看器仍会按 orientation=6 旋转，图片不会被转歪。
    assert_eq!(exif_orientation(&exif.payload[6..]), Some(6));
    let icc_kept = segments
        .iter()
        .any(|s| s.marker == 0xE2 && s.payload.windows(icc.len()).any(|w| w == icc));
    assert!(icc_kept, "APP2/ICC 色彩配置应保留，否则会偏色");

    // 注入不能破坏 JPEG 本身：APP0 仍在最前，且能正常解码出原尺寸。
    assert_eq!(&out[0..2], &[0xFF, 0xD8]);
    assert_eq!(&out[2..4], &[0xFF, 0xE0]);
    assert_eq!(decode_dimensions(&out), (W, H));
}

#[test]
fn jpeg_without_metadata_stays_clean_and_valid() {
    let source = plain_jpeg(95);
    let out = compress("jpg", &source, &CompressionSettings::default());
    assert!(jpeg_metadata_segments(&out).is_empty());
    assert_eq!(decode_dimensions(&out), (W, H));
}

#[test]
fn jpeg_metadata_is_preserved_even_when_resized() {
    let icc = b"icc-after-resize-000111222";
    let source = jpeg_with_metadata(3, icc, 95);
    let settings = CompressionSettings {
        image_max_width_enabled: true,
        image_max_width: 12,
        ..Default::default()
    };
    let out = compress("jpg", &source, &settings);

    assert_eq!(decode_dimensions(&out), (12, 8));
    let segments = jpeg_metadata_segments(&out);
    let exif = segments.iter().find(|s| s.marker == 0xE1).unwrap();
    assert_eq!(exif_orientation(&exif.payload[6..]), Some(3));
}

#[test]
fn png_text_and_color_chunks_survive_reencode() {
    // 只用解码器能安全解析的块：iCCP / zTXt / eXIf 的原样搬运由纯函数测试覆盖。
    let chunks = vec![
        PngMetadataChunk { kind: *b"tEXt", data: b"Author\0Long".to_vec() },
        PngMetadataChunk { kind: *b"iTXt", data: b"Title\0\0\0\0\0hello".to_vec() },
        PngMetadataChunk { kind: *b"sRGB", data: vec![0] },
        PngMetadataChunk { kind: *b"gAMA", data: 0x0000_B18Fu32.to_be_bytes().to_vec() },
        PngMetadataChunk { kind: *b"cHRM", data: vec![0u8; 32] },
        PngMetadataChunk { kind: *b"tIME", data: vec![0u8; 7] },
        PngMetadataChunk { kind: *b"pHYs", data: vec![0u8; 9] },
    ];
    let source = png_with_metadata(&chunks);
    let out = compress("png", &source, &CompressionSettings::default());

    assert_eq!(png_metadata_chunks(&out, true), chunks);
    // 注入块位置与 CRC 都必须合法，否则解码器会拒绝整个 PNG。
    assert_eq!(decode_dimensions(&out), (W, H));
}

#[test]
fn png_phys_chunk_is_dropped_when_image_is_resized() {
    let chunks = vec![
        PngMetadataChunk { kind: *b"tEXt", data: b"Author\0Long".to_vec() },
        PngMetadataChunk { kind: *b"pHYs", data: vec![0u8; 9] },
    ];
    let source = png_with_metadata(&chunks);
    let settings = CompressionSettings {
        image_max_width_enabled: true,
        image_max_width: 12,
        ..Default::default()
    };
    let out = compress("png", &source, &settings);

    let kept = png_metadata_chunks(&out, true);
    assert_eq!(kept.len(), 1, "缩放后 pHYs/DPI 已失效，不应保留");
    assert_eq!(kept[0].kind, *b"tEXt");
    assert_eq!(decode_dimensions(&out), (12, 8));
}

#[test]
fn gif_finite_loop_count_is_preserved() {
    let palette = [255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0];
    let pixels = [0u8, 1, 2, 3, 0, 1, 2, 3];
    let source = gif_bytes(Some(Repeat::Finite(3)), &palette, &pixels, None);
    assert_eq!(gif_decoder(&source).repeat(), Repeat::Finite(3));

    let out = compress("gif", &source, &gif_settings());
    assert_eq!(gif_decoder(&out).repeat(), Repeat::Finite(3));
}

#[test]
fn gif_infinite_loop_count_is_preserved() {
    let palette = [255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0];
    let pixels = [0u8, 1, 2, 3, 0, 1, 2, 3];
    let source = gif_bytes(Some(Repeat::Infinite), &palette, &pixels, None);
    let out = compress("gif", &source, &gif_settings());
    assert_eq!(gif_decoder(&out).repeat(), Repeat::Infinite);
}

#[test]
fn gif_without_loop_extension_still_plays_once() {
    let palette = [255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0];
    let pixels = [0u8, 1, 2, 3, 0, 1, 2, 3];
    let source = gif_bytes(None, &palette, &pixels, None);
    assert_eq!(gif_decoder(&source).repeat(), Repeat::Finite(0));

    let out = compress("gif", &source, &gif_settings());
    assert_eq!(gif_decoder(&out).repeat(), Repeat::Finite(0));
    assert!(!out.windows(11).any(|w| w == b"NETSCAPE2.0"), "不应凭空写入循环扩展");
}

#[test]
fn gif_transparent_index_survives_palette_optimization() {
    // 索引 2 与 3 都是黑色：3 是透明索引，2 是实体黑。优化调色板会去重黑色，
    // 旧逻辑让两类像素落到同一槽位，把实体黑也挖成透明。
    let palette = [255, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0];
    let pixels = [2u8, 3, 0, 1, 2, 3, 0, 1];
    let source = gif_bytes(Some(Repeat::Finite(2)), &palette, &pixels, Some(3));
    let out = compress("gif", &source, &gif_settings());

    let mut decoder = gif_decoder(&out);
    let frame = decoder.read_next_frame().unwrap().unwrap();
    let slot = frame.transparent.expect("透明索引必须保留");
    assert_eq!(frame.buffer[1], slot, "原本透明的像素应指向透明槽");
    assert_eq!(frame.buffer[5], slot);
    assert_ne!(frame.buffer[0], slot, "实体黑像素不能被挖成透明");
    assert_ne!(frame.buffer[4], slot);
    assert_eq!(decoder.repeat(), Repeat::Finite(2), "循环次数应同时保留");
}

const CT: &[u8] = b"<?xml version=\"1.0\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"jpg\" ContentType=\"image/jpeg\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"gif\" ContentType=\"image/gif\"/></Types>";
const PRESENTATION: &[u8] = b"<?xml version=\"1.0\"?><p:presentation xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"/>";

#[test]
fn full_pipeline_keeps_media_metadata_in_output_pptx() {
    let dir = tempdir().unwrap();
    let pptx = dir.path().join("source.pptx");
    let jpeg = jpeg_with_metadata(8, b"icc-in-pptx-abcdefghijk", 95);
    let png = png_with_metadata(&[PngMetadataChunk {
        kind: *b"tEXt",
        data: b"Author\0Long".to_vec(),
    }]);
    let palette = [255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0];
    let gif = gif_bytes(Some(Repeat::Finite(4)), &palette, &[0u8, 1, 2, 3, 0, 1, 2, 3], None);

    {
        let mut zip = ZipWriter::new(fs::File::create(&pptx).unwrap());
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let entries: [(&str, &[u8]); 5] = [
            ("[Content_Types].xml", CT),
            ("ppt/presentation.xml", PRESENTATION),
            ("ppt/media/image1.jpg", &jpeg),
            ("ppt/media/image1.png", &png),
            ("ppt/media/anim1.gif", &gif),
        ];
        for (name, data) in entries {
            zip.start_file(name, stored).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }
    let original_bytes = fs::read(&pptx).unwrap();

    let settings = CompressionSettings {
        image_quality: 40, // 确保候选更小，触发替换路径
        gif_frame_drop: false,
        ..Default::default()
    };

    let (tx, rx) = mpsc::channel();
    run_batch(
        std::slice::from_ref(&pptx),
        Some(dir.path()),
        &settings,
        tx,
        Arc::new(AtomicBool::new(false)),
    );
    let messages: Vec<BatchMessage> = rx.try_iter().collect();
    let output: PathBuf = messages
        .iter()
        .find_map(|m| match m {
            BatchMessage::JobFinished(_, _, _, path) => Some(path.clone()),
            _ => None,
        })
        .expect("批处理应产出结果文件");
    assert!(messages.iter().any(|m| matches!(m, BatchMessage::Finished)));

    let mut archive = ZipArchive::new(fs::File::open(&output).unwrap()).unwrap();
    assert!(archive.by_name("[Content_Types].xml").is_ok());

    let mut jpg = Vec::new();
    archive
        .by_name("ppt/media/image1.jpg")
        .unwrap()
        .read_to_end(&mut jpg)
        .unwrap();
    let exif = jpeg_metadata_segments(&jpg)
        .into_iter()
        .find(|s| s.marker == 0xE1)
        .expect("输出 PPTX 内的 JPEG 应保留 EXIF");
    assert_eq!(exif_orientation(&exif.payload[6..]), Some(8));

    let mut png_out = Vec::new();
    archive
        .by_name("ppt/media/image1.png")
        .unwrap()
        .read_to_end(&mut png_out)
        .unwrap();
    assert!(png_metadata_chunks(&png_out, true)
        .iter()
        .any(|c| c.kind == *b"tEXt"));

    let mut gif_out = Vec::new();
    archive
        .by_name("ppt/media/anim1.gif")
        .unwrap()
        .read_to_end(&mut gif_out)
        .unwrap();
    assert_eq!(gif_decoder(&gif_out).repeat(), Repeat::Finite(4));

    // 无论候选被采纳还是保留原件，源文件都不应被改动。
    assert_eq!(fs::read(&pptx).unwrap(), original_bytes);
    assert_ne!(output, pptx, "结果必须写入新文件");
}
