//! 左侧主导区下半部：包内媒体清单、体积预估与逐项开关。

use crate::app::{
    AppState, PptxCompressorApp, ACCENT, ACCENT_SOFT, AMBER, AMBER_SOFT, GREEN,
    GREEN_SOFT, LINE, MUTED, RED, RED_SOFT, SURFACE, TEXT, WHITE,
};
use crate::core::{MediaStatus, MediaType};
use crate::ui::components;

pub fn show(ui: &mut egui::Ui, app: &mut PptxCompressorApp) {
    let state = app.state;
    let total_original: u64 = app.media_files.iter().map(|m| m.original_size).sum();
    let only_compressible = app.show_only_compressible;
    let filter_label = if only_compressible {
        "显示全部"
    } else {
        "仅看可压缩"
    };

    let subtitle = if app.is_batch() {
        format!("批量队列 · {} 个文稿", app.batch_len())
    } else {
        format!(
            "{} 个文件 · 合计 {}",
            app.media_files.len(),
            components::fmt_size(total_original)
        )
    };
    components::section_head(
        ui,
        "媒体内容",
        &subtitle,
        |ui| {
            if components::text_link(ui, filter_label).clicked() {
                app.show_only_compressible = !only_compressible;
            }
        },
    );

    if app.media_files.is_empty() {
        let hint = if app.is_batch() {
            "批量队列模式：每个文稿在开始后独立解析，不预先展开媒体清单"
        } else if state == AppState::Loading {
            "正在解析文稿，媒体清单稍后显示…"
        } else {
            "选择文件后，这里会列出 PPTX 内的所有媒体"
        };
        components::card(ui, SURFACE, LINE, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(26.0);
                ui.label(egui::RichText::new(hint).color(MUTED).size(11.5));
                ui.add_space(26.0);
            });
        });
        return;
    }

    if state == AppState::Ready {
        show_estimate(ui, app, total_original);
        ui.add_space(10.0);
    }

    let settings = app.settings.clone();
    let log = app.activity_log.clone();
    let mut notes: Vec<(String, &'static str, egui::Color32)> = Vec::new();

    egui::Frame::none()
        .fill(WHITE)
        .rounding(10.0)
        .stroke(egui::Stroke::new(1.0, LINE))
        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
        .show(ui, |ui| {
            egui::Grid::new("media_table")
                .num_columns(6)
                .spacing([10.0, 5.0])
                .min_col_width(38.0)
                .show(ui, |ui| {
                    for header in &["文件", "类型", "原始大小", "节省", "状态", "处理"] {
                        ui.label(
                            egui::RichText::new(*header)
                                .color(MUTED)
                                .size(10.5)
                                .strong(),
                        );
                    }
                    ui.end_row();

                    for media in app.media_files.iter_mut() {
                        if only_compressible && !media.media_type.is_compressible() {
                            continue;
                        }
                        let name = media
                            .extracted_path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                        let short = shorten(&name, 20);

                        ui.label(
                            egui::RichText::new(&short).color(TEXT).size(11.0),
                        );

                        let (type_text, type_fg, type_bg) = type_badge(media.media_type);
                        components::badge(ui, type_text, type_fg, type_bg);

                        ui.label(
                            egui::RichText::new(components::fmt_size(media.original_size))
                                .color(TEXT)
                                .size(11.0),
                        );

                        let (saving_text, saving_color) = saving_text(media, &settings);
                        ui.label(
                            egui::RichText::new(saving_text)
                                .color(saving_color)
                                .size(11.0)
                                .strong(),
                        );

                        let (status_text, status_fg, status_bg) = status_badge(media.status);
                        components::badge(ui, status_text, status_fg, status_bg);

                        if state != AppState::Compressing && media.media_type.is_compressible() {
                            ui.checkbox(&mut media.enabled, "");
                        } else {
                            ui.label(egui::RichText::new("—").color(MUTED).size(11.0));
                        }
                        ui.end_row();

                        if matches!(media.status, MediaStatus::Skipped | MediaStatus::Failed) {
                            let reason = reason_for(&log, &name, media.status);
                            let label = if media.status == MediaStatus::Failed {
                                "失败"
                            } else {
                                "保留原件"
                            };
                            let color = if media.status == MediaStatus::Failed {
                                RED
                            } else {
                                AMBER
                            };
                            notes.push((format!("{} · {}：{}", name, label, reason), label, color));
                        }
                    }
                });
        });

    // 逐项原因集中列在表格下方，避免撑宽列或遮挡相邻单元格。
    for (text, label, color) in notes {
        ui.add_space(4.0);
        let bg = if label == "失败" { RED_SOFT } else { AMBER_SOFT };
        egui::Frame::none()
            .fill(bg)
            .rounding(6.0)
            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
            .show(ui, |ui| {
                ui.label(egui::RichText::new(text).color(color).size(10.5));
            });
    }
}

fn show_estimate(ui: &mut egui::Ui, app: &PptxCompressorApp, total_original: u64) {
    let estimated: u64 = app
        .media_files
        .iter()
        .map(|m| m.estimated_size(&app.settings))
        .sum();
    let saved = total_original.saturating_sub(estimated);
    let pct = if total_original > 0 {
        (saved as f64 / total_original as f64 * 100.0) as u8
    } else {
        0
    };

    egui::Frame::none()
        .fill(ACCENT_SOFT)
        .rounding(10.0)
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(221, 231, 255)))
        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "预计可节省约 {}（{}%）",
                        components::fmt_size(saved),
                        pct
                    ))
                    .color(crate::app::ACCENT_DARK)
                    .size(12.0)
                    .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("按当前参数估算，实际结果以压缩完成为准")
                            .color(crate::app::TEXT_2)
                            .size(10.5),
                    );
                });
            });
        });
}

fn saving_text(
    media: &crate::core::MediaInfo,
    settings: &crate::core::CompressionSettings,
) -> (String, egui::Color32) {
    match media.status {
        MediaStatus::Done if media.compressed_size > 0 => {
            let saved = media.original_size.saturating_sub(media.compressed_size);
            let pct = if media.original_size > 0 {
                saved as f64 / media.original_size as f64 * 100.0
            } else {
                0.0
            };
            (format!("-{}%", pct as u8), GREEN)
        }
        MediaStatus::Done => ("—".into(), MUTED),
        MediaStatus::Skipped => ("保留原件".into(), AMBER),
        MediaStatus::Failed => ("—".into(), RED),
        MediaStatus::Processing => ("计算中…".into(), ACCENT),
        MediaStatus::Pending => {
            if !media.enabled {
                return ("不处理".into(), MUTED);
            }
            let estimate = media.estimated_size(settings);
            let pct = if media.original_size > 0 {
                (media.original_size.saturating_sub(estimate)) as f64
                    / media.original_size as f64
                    * 100.0
            } else {
                0.0
            };
            (format!("≈ -{}%", pct as u8), MUTED)
        }
    }
}

fn status_badge(status: MediaStatus) -> (&'static str, egui::Color32, egui::Color32) {
    match status {
        MediaStatus::Pending => ("待处理", MUTED, SURFACE),
        MediaStatus::Processing => ("处理中", ACCENT, ACCENT_SOFT),
        MediaStatus::Done => ("已完成", GREEN, GREEN_SOFT),
        MediaStatus::Skipped => ("保留原件", AMBER, AMBER_SOFT),
        MediaStatus::Failed => ("失败", RED, RED_SOFT),
    }
}

fn type_badge(kind: MediaType) -> (&'static str, egui::Color32, egui::Color32) {
    match kind {
        MediaType::Jpeg => ("JPEG", egui::Color32::from_rgb(63, 98, 201), ACCENT_SOFT),
        MediaType::Png => ("PNG", egui::Color32::from_rgb(63, 98, 201), ACCENT_SOFT),
        MediaType::Gif => ("GIF", egui::Color32::from_rgb(180, 110, 20), AMBER_SOFT),
        MediaType::Video => ("视频", egui::Color32::from_rgb(14, 140, 124), egui::Color32::from_rgb(234, 246, 244)),
        MediaType::Other => ("其他", MUTED, SURFACE),
    }
}

/// 从活动日志中找出与该文件相关的最后一条说明。
fn reason_for(log: &[String], name: &str, status: MediaStatus) -> String {
    log.iter()
        .rev()
        .find(|line| line.contains(name))
        .cloned()
        .unwrap_or_else(|| {
            if status == MediaStatus::Failed {
                "处理失败，已保留原始文件，详见右侧活动日志".into()
            } else {
                "重编码后体积未减小或该格式需额外设置，已自动保留原文件".into()
            }
        })
}

fn shorten(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return text.to_owned();
    }
    let head: String = chars[..max_chars.saturating_sub(6)].iter().collect();
    let tail: String = chars[chars.len() - 5..].iter().collect();
    format!("{}…{}", head, tail)
}
