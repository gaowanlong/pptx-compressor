//! FFmpeg 合规判定与双编码器参数构造（不需要本机安装 FFmpeg）。

use pptx_compressor::core::CompressionSettings;
use pptx_compressor::handlers::video::{bitrate_kbps, build_video_args};
use pptx_compressor::utils::ffmpeg::{
    license_from_configuration, EncoderSupport, LicenseClass,
};
use std::path::Path;

#[test]
fn license_gate_rejects_gpl_and_nonfree_builds() {
    assert_eq!(
        license_from_configuration("configuration: --enable-nonfree --enable-gpl --enable-libx264"),
        LicenseClass::Nonfree
    );
    assert_eq!(
        license_from_configuration("configuration: --enable-gpl --enable-version3 --enable-libx264"),
        LicenseClass::Gpl
    );
    assert_eq!(
        license_from_configuration("configuration: --enable-version3"),
        LicenseClass::Gpl
    );
    let lgpl = license_from_configuration(
        "configuration: --disable-gpl --disable-nonfree --enable-libopenh264",
    );
    assert_eq!(lgpl, LicenseClass::Lgpl);
    assert!(lgpl.redistributable());
    assert!(!LicenseClass::Gpl.redistributable());
    assert!(!LicenseClass::Nonfree.redistributable());
}

#[test]
fn encoder_preference_falls_back_to_openh264() {
    let full = EncoderSupport { x264: true, openh264: true };
    assert_eq!(full.preferred(), Some("libx264"));
    let lgpl = EncoderSupport { x264: false, openh264: true };
    assert_eq!(lgpl.preferred(), Some("libopenh264"));
    let none = EncoderSupport { x264: false, openh264: false };
    assert_eq!(none.preferred(), None);
}

#[test]
fn x264_arguments_use_crf_and_high_profile() {
    let settings = CompressionSettings::default();
    let args = build_video_args(
        Path::new("in.mov"),
        Path::new("out.mp4"),
        &settings,
        "libx264",
    )
    .unwrap();
    assert_eq!(args[4], "libx264");
    assert!(args.windows(2).any(|w| w == ["-crf", "29"]));
    assert!(args.windows(2).any(|w| w == ["-preset", "medium"]));
    assert!(args.windows(2).any(|w| w == ["-profile:v", "high"]));
    assert!(args.contains(&"yuv420p".to_owned()));
    assert!(args.contains(&"+faststart".to_owned()));
    assert_eq!(args.last().unwrap(), "out.mp4");
}

#[test]
fn openh264_arguments_use_bitrate_and_no_x264_only_flags() {
    let settings = CompressionSettings::default();
    let args = build_video_args(
        Path::new("in.mov"),
        Path::new("out.mp4"),
        &settings,
        "libopenh264",
    )
    .unwrap();
    assert_eq!(args[4], "libopenh264");
    assert!(args.windows(2).any(|w| w[0] == "-b:v" && w[1] == format!("{}k", bitrate_kbps(settings.video_quality))));
    for forbidden in ["-crf", "-preset", "-profile:v", "-refs"] {
        assert!(!args.contains(&forbidden.to_owned()), "LGPL 构建不应带 {forbidden}");
    }
    assert_eq!(args.last().unwrap(), "out.mp4");
}

#[test]
fn unsupported_encoder_is_rejected_and_quality_changes_bitrate() {
    let settings = CompressionSettings::default();
    assert!(build_video_args(Path::new("a"), Path::new("b"), &settings, "mpeg4").is_err());
    assert!(bitrate_kbps(100) > bitrate_kbps(45));
    assert!(bitrate_kbps(45) > bitrate_kbps(0));
}

#[test]
fn resolution_limit_is_applied_for_both_encoders() {
    let settings = CompressionSettings {
        video_max_resolution_enabled: true,
        video_max_height: 720,
        ..CompressionSettings::default()
    };
    for encoder in ["libx264", "libopenh264"] {
        let args = build_video_args(
            Path::new("in.mp4"),
            Path::new("out.mp4"),
            &settings,
            encoder,
        )
        .unwrap();
        assert!(
            args.windows(2).any(|w| w[0] == "-vf" && w[1].contains("min(ih,720)")),
            "{encoder} 缺少缩放滤镜"
        );
        assert!(!args.contains(&"yuv420p".to_owned()));
    }
}
