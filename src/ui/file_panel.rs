//! 左侧主导区上半部：源文件选择、拖放与解析状态。
//!
//! 解压在后台线程进行，GUI 只负责派发任务与展示结果，超大 PPTX 不会冻结窗口。

use crate::app::{AppState, LoadMessage, PptxCompressorApp, ACCENT, ACCENT_SOFT, AMBER, AMBER_SOFT, GREEN, GREEN_SOFT, LINE, MUTED, SURFACE_2, TEXT, WHITE};
use crate::core::{extractor::extract_pptx_inspected, MediaInfo};
use crate::ui::components;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

pub fn show(ui: &mut egui::Ui, app: &mut PptxCompressorApp) {
    let hovering = ui.input(|i| !i.raw.hovered_files.is_empty());

    components::section_head(ui, "源文件", "自动检测加密 / 签名 / 宏", |ui| {
        if !app.input_files.is_empty()
            && app.state != AppState::Compressing
            && components::text_link(ui, "更换文件").clicked()
        {
            pick_file(app);
        }
    });

    if app.input_files.is_empty() {
        show_drop_zone(ui, app, hovering);
    } else {
        show_file_card(ui, app);
    }

    // 拖放只在有 drop 事件的那一帧产生，不会重复触发加载。
    let dropped = ui.input(|i| {
        i.raw
            .dropped_files
            .first()
            .and_then(|f| f.path.clone())
    });
    if let Some(path) = dropped {
        if app.state == AppState::Compressing {
            app.log("压缩进行中，暂不接收新文件");
        } else if is_pptx(&path) {
            load_pptx(app, Some(&path));
        } else {
            app.log("已忽略：仅支持 .pptx 文件");
        }
    }
}

fn is_pptx(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("pptx"))
        .unwrap_or(false)
}

fn pick_file(app: &mut PptxCompressorApp) {
    if let Some(file) = rfd::FileDialog::new()
        .add_filter("PowerPoint 演示文稿", &["pptx"])
        .pick_file()
    {
        load_pptx(app, Some(&file));
    }
}

fn show_drop_zone(ui: &mut egui::Ui, app: &mut PptxCompressorApp, hovering: bool) {
    let border = if hovering {
        ACCENT
    } else {
        egui::Color32::from_rgb(205, 214, 232)
    };
    let fill = if hovering { ACCENT_SOFT } else { SURFACE_2 };

    egui::Frame::none()
        .fill(fill)
        .rounding(14.0)
        .stroke(egui::Stroke::new(1.5, border))
        .inner_margin(egui::Margin::symmetric(20.0, 30.0))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                egui::Frame::none()
                    .fill(ACCENT_SOFT)
                    .rounding(12.0)
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("↓").color(ACCENT).size(22.0).strong(),
                        );
                    });
                ui.add_space(14.0);
                ui.label(
                    egui::RichText::new("把 PPTX 拖到这里")
                        .color(TEXT)
                        .size(15.0)
                        .strong(),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("一次处理一个文件 · 支持图片、GIF 与视频")
                        .color(MUTED)
                        .size(11.5),
                );
                ui.add_space(16.0);

                let btn = egui::Button::new(
                    egui::RichText::new("选择 PPTX 文件")
                        .color(WHITE)
                        .size(12.0)
                        .strong(),
                )
                .fill(ACCENT)
                .stroke(egui::Stroke::NONE)
                .rounding(9.0)
                .min_size(egui::vec2(0.0, 34.0));
                if ui.add(btn).clicked() {
                    pick_file(app);
                }

                ui.add_space(14.0);
                ui.label(
                    egui::RichText::new(
                        "原文件不会被修改，结果另存为「原文件名_compressed.pptx」",
                    )
                    .color(MUTED)
                    .size(10.5),
                );
                ui.label(
                    egui::RichText::new("若目标文件已存在，会自动追加序号，绝不覆盖")
                        .color(MUTED)
                        .size(10.5),
                );
            });
        });
}

fn show_file_card(ui: &mut egui::Ui, app: &mut PptxCompressorApp) {
    let Some(file) = app.input_files.first().cloned() else {
        return;
    };
    let name = file
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let size = std::fs::metadata(&file)
        .map(|m| components::fmt_size(m.len()))
        .unwrap_or_else(|_| "未知大小".into());
    let parent = file
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let media_count = app.media_files.len();
    let loading = app.state == AppState::Loading;

    components::card(ui, WHITE, LINE, |ui| {
        ui.horizontal(|ui| {
            egui::Frame::none()
                .fill(ACCENT_SOFT)
                .rounding(10.0)
                .inner_margin(egui::Margin::symmetric(11.0, 9.0))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("P").color(ACCENT).size(16.0).strong(),
                    );
                });
            ui.add_space(12.0);
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(&name).color(TEXT).size(13.5).strong(),
                );
                ui.add_space(4.0);
                let detail = if media_count > 0 {
                    format!("{} · {} 个内嵌媒体 · {}", size, media_count, shorten(&parent, 46))
                } else {
                    format!("{} · {}", size, shorten(&parent, 56))
                };
                ui.label(egui::RichText::new(detail).color(MUTED).size(11.0));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if loading {
                    ui.spinner();
                    ui.add_space(6.0);
                    components::badge(ui, "解析中", ACCENT, ACCENT_SOFT);
                } else {
                    components::badge(ui, "已解析", GREEN, GREEN_SOFT);
                }
            });
        });
    });

    // 输入安全检测的告警最多直接展示两条，完整列表写入活动日志。
    for warning in app.input_warnings.iter().take(2) {
        ui.add_space(6.0);
        components::notice(ui, &format!("输入检查 · {}", warning), AMBER, AMBER_SOFT);
    }
    if app.input_warnings.len() > 2 {
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(format!(
                "另有 {} 项提示，详见活动日志",
                app.input_warnings.len() - 2
            ))
            .color(MUTED)
            .size(10.5),
        );
    }

    ui.add_space(8.0);
    if app.state != AppState::Compressing {
        ui.horizontal(|ui| {
            if components::text_link(ui, "移除文件").clicked() {
                app.reset_for_new_file();
            }
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("更换文件会重新解压，旧结果不会保留")
                    .color(MUTED)
                    .size(10.5),
            );
        });
    }
}

fn shorten(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return text.to_owned();
    }
    let head: String = chars[..max_chars / 2].iter().collect();
    let tail: String = chars[chars.len() - max_chars / 2..].iter().collect();
    format!("{}…{}", head, tail)
}

/// 在后台线程解压 PPTX，结果通过消息通道回传。
///
/// `load_generation` 用于丢弃过期结果：用户在解析期间换了文件时，
/// 旧线程的结果不会被写回状态，其临时目录也会被立即清理。
pub fn load_pptx(app: &mut PptxCompressorApp, source: Option<&Path>) {
    let Some(pptx_path) = source
        .map(|p| p.to_path_buf())
        .or_else(|| app.input_files.first().cloned())
    else {
        return;
    };

    app.input_files = vec![pptx_path.clone()];
    app.load_generation = app.load_generation.wrapping_add(1);
    let generation = app.load_generation;

    app.state = AppState::Loading;
    app.activity_log.clear();
    app.input_warnings.clear();
    app.media_files.clear();
    app._temp_dir = None;
    app.extract_dir = None;
    app.output_path = None;
    app.rx = None;
    app.result_original = 0;
    app.result_compressed = 0;
    app.progress_current = 0;
    app.progress_total = 0;
    app.progress_file.clear();

    let (tx, rx) = mpsc::channel();
    app.load_rx = Some(rx);

    std::thread::spawn(move || {
        let outcome: Result<
            (Vec<MediaInfo>, PathBuf, tempfile::TempDir, Vec<String>),
            String,
        > = (|| {
            let temp = tempfile::tempdir()
                .map_err(|e| format!("创建临时目录失败：{}", e))?;
            let extract_path = temp.path().to_path_buf();
            let (media, inspection) = extract_pptx_inspected(&pptx_path, &extract_path)
                .map_err(|e| e.to_string())?;
            Ok((
                media,
                extract_path,
                temp,
                inspection.warning_messages(),
            ))
        })();

        match outcome {
            Ok((media, dir, temp, warnings)) => {
                let _ = tx.send(LoadMessage::Loaded(generation, media, dir, temp, warnings));
            }
            Err(e) => {
                let _ = tx.send(LoadMessage::Failed(generation, e));
            }
        }
    });
}
