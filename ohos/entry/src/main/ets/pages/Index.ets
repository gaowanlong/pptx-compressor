import {
  AppState,
  Theme,
  formatBytes,
  kindLabel,
  stateBg,
  stateColor,
  stateLabel,
  statusLabel,
} from '../common/Theme';
import { NativeBridge } from '../model/Bridge';
import {
  BatchRequest,
  BridgeEvent,
  BridgeMedia,
  Capabilities,
  CompressRequest,
  CompressionSettings,
  InspectRequest,
  MediaComposition,
  PersistedState,
  PollReply,
  SettingsPatch,
  StartReply,
  DEFAULT_SETTINGS,
} from '../model/Protocol';
import { SandboxFiles } from '../model/SandboxFiles';
import { SettingsStore } from '../model/SettingsStore';

@Entry
@Component
struct Index {
  @State appState: AppState = AppState.Idle;
  @State wide: boolean = true;
  @State inputPaths: string[] = [];
  @State sourceName: string = '';
  @State sourceSize: number = 0;
  @State media: BridgeMedia[] = [];
  @State warnings: string[] = [];
  @State composition: MediaComposition | null = null;
  @State settings: CompressionSettings = DEFAULT_SETTINGS;
  @State outputDir: string | null = null;
  @State showAdvanced: boolean = false;
  @State caps: Capabilities | null = null;
  @State progress: number = 0;
  @State progressFile: string = '';
  @State resultOriginal: number = 0;
  @State resultCompressed: number = 0;
  @State outputPath: string = '';
  @State activityLog: string[] = [];
  @State errorText: string = '';
  @State hint: string = '';
  @State sessionId: number = 0;
  @State jobId: number = 0;
  @State queueDone: number = 0;

  private timerId: number = -1;
  private contextRef: Context | undefined = undefined;

  async aboutToAppear(): Promise<void> {
    const ctx: Context = getContext(this);
    this.contextRef = ctx;
    SandboxFiles.ensureDirs(ctx);

    const handshake: string = NativeBridge.handshake();
    if (handshake.length > 0) {
      this.fail(handshake);
      return;
    }
    const initReply = NativeBridge.init(ctx.filesDir, ctx.cacheDir, SandboxFiles.tempDir(ctx));
    if (!initReply.ok || initReply.data === undefined) {
      this.fail(initReply.error ?? '沙箱初始化失败');
      return;
    }
    this.caps = initReply.data;

    await SettingsStore.init(ctx);
    const saved: PersistedState = await SettingsStore.load();
    this.settings = saved.settings;
    this.outputDir = saved.output_dir;
    this.showAdvanced = saved.show_advanced;
  }

  aboutToDisappear(): void {
    this.stopPolling();
  }

  // ── 状态与事件 ──────────────────────────────────────────

  private fail(message: string): void {
    this.errorText = message;
    this.appState = AppState.Error;
    this.log(message);
  }

  private log(line: string): void {
    const copy: string[] = [...this.activityLog];
    copy.push(line);
    if (copy.length > 200) {
      copy.shift();
    }
    this.activityLog = copy;
  }

  private stopPolling(): void {
    if (this.timerId >= 0) {
      clearInterval(this.timerId);
      this.timerId = -1;
    }
  }

  private startPolling(): void {
    this.stopPolling();
    this.timerId = setInterval(() => {
      const response = NativeBridge.pollEvents(this.jobId);
      if (!response.ok || response.data === undefined) {
        this.stopPolling();
        this.fail(response.error ?? '事件轮询失败');
        return;
      }
      const reply: PollReply = response.data;
      for (const event of reply.events) {
        this.applyEvent(event);
      }
      if (reply.done) {
        this.stopPolling();
        NativeBridge.dropJob(this.jobId);
        this.jobId = 0;
      }
    }, 120);
  }

  private applyEvent(event: BridgeEvent): void {
    switch (event.type) {
      case 'inspected':
        this.sessionId = event.session_id ?? 0;
        this.sourceName = SandboxFiles.fileName(event.source_path ?? this.sourceName);
        this.sourceSize = event.source_size ?? 0;
        this.media = event.media ?? [];
        this.warnings = event.warnings ?? [];
        this.composition = event.composition ?? null;
        this.appState = AppState.Ready;
        break;
      case 'status_update': {
        const idx: number = event.index ?? -1;
        if (idx >= 0 && idx < this.media.length) {
          const copy: BridgeMedia[] = [...this.media];
          const old: BridgeMedia = copy[idx];
          const updated: BridgeMedia = {
            index: old.index,
            path: old.path,
            name: old.name,
            kind: old.kind,
            original_size: old.original_size,
            enabled: old.enabled,
            status: event.status ?? 'pending',
          };
          copy[idx] = updated;
          this.media = copy;
        }
        break;
      }
      case 'job_started':
        this.progressFile = event.name ?? '';
        this.log(`开始处理 ${event.name ?? ''}（${event.media_count ?? 0} 个媒体）`);
        for (const warning of event.warnings ?? []) {
          this.log(`告警：${warning}`);
        }
        break;
      case 'job_progress': {
        const current: number = event.current ?? 0;
        const total: number = Math.max(event.total ?? 1, 1);
        const files: number = Math.max(this.inputPaths.length, 1);
        this.progress = Math.min(100, Math.round(
          ((this.queueDone + current / total) / files) * 100,
        ));
        this.progressFile = event.file ?? '';
        break;
      }
      case 'job_log':
        this.log(event.message ?? '');
        break;
      case 'job_finished':
        this.queueDone += 1;
        this.resultOriginal = event.original ?? 0;
        this.resultCompressed = event.compressed ?? 0;
        this.outputPath = event.output_path ?? '';
        this.progress = 100;
        this.appState = AppState.Done;
        this.hint = '压缩完成，可导出结果文件';
        break;
      case 'job_failed':
        this.log(`失败：${event.message ?? ''}`);
        this.errorText = event.message ?? '处理失败';
        if (this.inputPaths.length <= 1) {
          this.appState = AppState.Error;
        }
        break;
      case 'finished': {
        const summary = event.summary;
        if (summary !== undefined) {
          this.resultOriginal = summary.total_original;
          this.resultCompressed = summary.total_compressed;
          this.log(`队列结束：成功 ${summary.succeeded} / 失败 ${summary.failed}`);
        }
        this.progress = 100;
        this.appState = AppState.Done;
        break;
      }
      case 'cancelled':
        this.appState = this.sessionId > 0 ? AppState.Ready : AppState.Idle;
        this.log('已取消，未生成输出文件');
        break;
      case 'fatal':
        this.fail(event.message ?? '内核错误');
        break;
      default:
        this.log(`未识别事件：${event.type}`);
        break;
    }
  }

  // ── 操作 ────────────────────────────────────────────────

  private patch(p: SettingsPatch): void {
    const s: CompressionSettings = this.settings;
    this.settings = {
      image_quality: p.image_quality ?? s.image_quality,
      video_quality: p.video_quality ?? s.video_quality,
      gif_palette_optimize: p.gif_palette_optimize ?? s.gif_palette_optimize,
      gif_frame_drop: p.gif_frame_drop ?? s.gif_frame_drop,
      gif_target_fps: p.gif_target_fps ?? s.gif_target_fps,
      image_max_width_enabled: p.image_max_width_enabled ?? s.image_max_width_enabled,
      image_max_width: p.image_max_width ?? s.image_max_width,
      video_max_resolution_enabled: p.video_max_resolution_enabled ?? s.video_max_resolution_enabled,
      video_max_height: p.video_max_height ?? s.video_max_height,
      video_convert_to_h264: p.video_convert_to_h264 ?? s.video_convert_to_h264,
    };
    this.saveSettings();
  }

  private async saveSettings(): Promise<void> {
    const state: PersistedState = {
      settings: this.settings,
      output_dir: this.outputDir,
      show_advanced: this.showAdvanced,
      ffmpeg_override: null,
    };
    await SettingsStore.save(state);
  }

  private async pickFiles(): Promise<void> {
    const ctx: Context | undefined = this.contextRef;
    if (ctx === undefined) {
      return;
    }
    this.stopPolling();
    if (this.sessionId > 0) {
      NativeBridge.dropSession(this.sessionId);
    }
    const paths: string[] = await SandboxFiles.pickAndImport(ctx);
    if (paths.length === 0) {
      return;
    }
    this.inputPaths = paths;
    this.media = [];
    this.warnings = [];
    this.composition = null;
    this.errorText = '';
    this.hint = '';
    this.activityLog = [];
    this.outputPath = '';
    this.progress = 0;
    this.queueDone = 0;
    this.sessionId = 0;
    this.resultOriginal = 0;
    this.resultCompressed = 0;
    this.sourceName = paths.length === 1 ? SandboxFiles.fileName(paths[0]) : `${paths.length} 个文稿`;

    if (paths.length > 1) {
      this.appState = AppState.Ready;
      this.hint = '批量模式：将逐个处理，单个失败不影响后续文件';
      return;
    }

    this.appState = AppState.Loading;
    const request: InspectRequest = { mode: 'inspect', path: paths[0] };
    this.launch(JSON.stringify(request));
  }

  private launch(requestJson: string): void {
    const reply = NativeBridge.startJob(requestJson);
    if (!reply.ok || reply.data === undefined) {
      this.fail(reply.error ?? '启动任务失败');
      return;
    }
    const data: StartReply = reply.data;
    this.jobId = data.job_id;
    this.startPolling();
  }

  private startCompression(): void {
    if (this.inputPaths.length === 0) {
      return;
    }
    this.errorText = '';
    this.hint = '';
    this.progress = 0;
    this.queueDone = 0;
    this.outputPath = '';
    this.appState = AppState.Compressing;

    if (this.inputPaths.length > 1) {
      const request: BatchRequest = {
        mode: 'batch',
        inputs: this.inputPaths,
        output_dir: this.outputDir,
        settings: this.settings,
      };
      this.launch(JSON.stringify(request));
      return;
    }
    const enabled: boolean[] = this.media.map((m: BridgeMedia): boolean => m.enabled);
    const request: CompressRequest = {
      mode: 'compress',
      session_id: this.sessionId,
      output_dir: this.outputDir,
      settings: this.settings,
      enabled: enabled,
    };
    this.launch(JSON.stringify(request));
  }

  private cancelJob(): void {
    if (this.jobId > 0) {
      NativeBridge.cancel(this.jobId);
      this.log('已请求取消…');
    }
  }

  private async exportResult(): Promise<void> {
    const ctx: Context | undefined = this.contextRef;
    if (ctx === undefined || this.outputPath.length === 0) {
      return;
    }
    const ok: boolean = await SandboxFiles.exportResult(
      ctx, this.outputPath, SandboxFiles.fileName(this.outputPath),
    );
    this.hint = ok ? '已导出到所选位置' : '导出未完成（可能已取消）';
  }

  private toggleMedia(index: number, value: boolean, kind: string): void {
    if (kind === 'video' && !this.videoAvailable()) {
      this.hint = '鸿蒙电脑版暂不提供视频转码，该项保持原样';
      return;
    }
    if (index < 0 || index >= this.media.length) {
      return;
    }
    const copy: BridgeMedia[] = [...this.media];
    const old: BridgeMedia = copy[index];
    const updated: BridgeMedia = {
      index: old.index,
      path: old.path,
      name: old.name,
      kind: old.kind,
      original_size: old.original_size,
      enabled: value,
      status: old.status,
    };
    copy[index] = updated;
    this.media = copy;
  }

  private applyPreset(preset: string): void {
    if (preset === 'high') {
      this.patch({
        image_quality: 90, video_quality: 65,
        image_max_width_enabled: false, image_max_width: 1920,
        video_max_resolution_enabled: false, video_max_height: 1080,
      });
    } else if (preset === 'balanced') {
      this.patch({
        image_quality: 75, video_quality: 45,
        image_max_width_enabled: false, image_max_width: 1920,
        video_max_resolution_enabled: false, video_max_height: 1080,
      });
    } else if (preset === 'small') {
      this.patch({
        image_quality: 30, video_quality: 18,
        image_max_width_enabled: true, image_max_width: 1024,
        video_max_resolution_enabled: true, video_max_height: 720,
      });
    }
  }

  private presetName(): string {
    const s: CompressionSettings = this.settings;
    if (s.image_quality === 90 && s.video_quality === 65 &&
      !s.image_max_width_enabled && !s.video_max_resolution_enabled) {
      return '高质量';
    }
    if (s.image_quality === 75 && s.video_quality === 45 &&
      !s.image_max_width_enabled && !s.video_max_resolution_enabled) {
      return '均衡';
    }
    if (s.image_quality === 30 && s.video_quality === 18 &&
      s.image_max_width_enabled && s.video_max_resolution_enabled) {
      return '最小体积';
    }
    return '自定义';
  }

  private videoAvailable(): boolean {
    const c: Capabilities | null = this.caps;
    return c === null || c.video_transcode;
  }

  private compositionMediaLine(): string {
    const c: MediaComposition | null = this.composition;
    return c === null ? '' : `媒体总体积 ${formatBytes(c.media_bytes)}，共 ${c.total_count} 项`;
  }

  private compositionBoundLine(): string {
    const c: MediaComposition | null = this.composition;
    return c === null ? ''
      : `可优化上界 ${formatBytes(c.optimizable_bytes)}（${c.optimizable_count} 项启用）`;
  }

  private capsVersion(): string {
    const c: Capabilities | null = this.caps;
    return c === null ? '未加载' : `${c.version} · 协议 v${c.protocol_version}`;
  }

  private capsPlatform(): string {
    const c: Capabilities | null = this.caps;
    return c === null ? '未知' : c.platform;
  }

  // ── 界面 ────────────────────────────────────────────────

  @Builder
  CardHeader(title: string) {
    Text(title)
      .fontSize(13)
      .fontColor(Theme.TEXT_2)
      .margin({ bottom: 10 })
  }

  @Builder
  LeftPane() {
    Scroll() {
      Column({ space: 12 }) {
        Column() {
          this.CardHeader('源文件')
          Text(this.sourceName.length > 0 ? this.sourceName : '尚未选择演示文稿')
            .fontSize(16).fontWeight(FontWeight.Medium).fontColor(Theme.TEXT)
          Text(this.sourceSize > 0 ? `文稿体积 ${formatBytes(this.sourceSize)}` : '支持 .pptx，最多一次 20 个')
            .fontSize(12).fontColor(Theme.MUTED).margin({ top: 4 })
          ForEach(this.warnings, (w: string, i: number) => {
            Text(`⚠ ${w}`)
              .fontSize(12).fontColor(Theme.AMBER)
              .backgroundColor('#FFFFF6E5').padding(8).borderRadius(6).width('100%')
          }, (w: string, i: number) => `warn-${i}`)
        }
        .width('100%').padding(16).backgroundColor(Theme.WHITE)
        .border({ width: 1, color: Theme.LINE, radius: 10 })
        .alignItems(HorizontalAlign.Start)

        if (this.composition !== null) {
          Column() {
            this.CardHeader('体积构成 · 可优化空间上界')
            Text(this.compositionMediaLine()).fontSize(12).fontColor(Theme.TEXT_2)
            Text(this.compositionBoundLine()).fontSize(12).fontColor(Theme.ACCENT_DARK)
            Text('说明：这是由真实字节数推出的数学上界，不是压缩结果预测。')
              .fontSize(11).fontColor(Theme.MUTED)
          }
          .width('100%').padding(16).backgroundColor(Theme.WHITE)
          .border({ width: 1, color: Theme.LINE, radius: 10 })
          .alignItems(HorizontalAlign.Start)
        }

        Column() {
          this.CardHeader(`媒体清单 · ${this.media.length} 项`)
          if (this.media.length === 0) {
            Text('选择文件后显示文稿内的图片、GIF 与视频')
              .fontSize(12).fontColor(Theme.MUTED)
          } else {
            ForEach(this.media, (item: BridgeMedia) => {
              Row() {
                Column({ space: 2 }) {
                  Text(item.name).fontSize(13).fontColor(Theme.TEXT)
                  Text(`${kindLabel(item.kind)} · ${formatBytes(item.original_size)} · ${statusLabel(item.status)}`)
                    .fontSize(11).fontColor(Theme.MUTED)
                }.alignItems(HorizontalAlign.Start).layoutWeight(1)
                Toggle({ type: ToggleType.Switch, isOn: item.enabled })
                  .onChange((isOn: boolean) => {
                    this.toggleMedia(item.index, isOn, item.kind);
                  })
              }.width('100%').padding({ top: 6, bottom: 6 })
            }, (item: BridgeMedia) => `media-${item.index}`)
          }
        }
        .width('100%').padding(16).backgroundColor(Theme.WHITE)
        .border({ width: 1, color: Theme.LINE, radius: 10 })
        .alignItems(HorizontalAlign.Start)

        if (this.inputPaths.length > 1) {
          Column() {
            this.CardHeader('批量队列')
            ForEach(this.inputPaths, (p: string, i: number) => {
              Text(`${i + 1}. ${SandboxFiles.fileName(p)}`)
                .fontSize(12).fontColor(i < this.queueDone ? Theme.GREEN : Theme.TEXT_2)
            }, (p: string, i: number) => `queue-${i}`)
          }
          .width('100%').padding(16).backgroundColor(Theme.WHITE)
          .border({ width: 1, color: Theme.LINE, radius: 10 })
          .alignItems(HorizontalAlign.Start)
        }
      }.width('100%').padding({ left: 16, right: 8, top: 12, bottom: 12 })
    }.layoutWeight(1)
  }

  @Builder
  RightPane() {
    Scroll() {
      Column({ space: 12 }) {
        Column() {
          this.CardHeader('压缩方案')
          Row({ space: 8 }) {
            Button('高质量').fontSize(12).onClick(() => { this.applyPreset('high'); })
            Button('均衡').fontSize(12).onClick(() => { this.applyPreset('balanced'); })
            Button('最小体积').fontSize(12).onClick(() => { this.applyPreset('small'); })
          }
          Text(`当前：${this.presetName()}`).fontSize(12).fontColor(Theme.TEXT_2)
            .margin({ top: 8, bottom: 8 })

          Text(`图片质量 ${this.settings.image_quality}`).fontSize(12).fontColor(Theme.TEXT)
          Slider({ value: this.settings.image_quality, min: 0, max: 100, step: 1 })
            .onChange((v: number) => { this.patch({ image_quality: Math.round(v) }); })

          Text(`视频质量 ${this.settings.video_quality}${this.videoAvailable() ? '' : '（鸿蒙版不可用）'}`)
            .fontSize(12).fontColor(Theme.TEXT)
          Slider({ value: this.settings.video_quality, min: 0, max: 100, step: 1 })
            .enabled(this.videoAvailable())
            .onChange((v: number) => { this.patch({ video_quality: Math.round(v) }); })

          Row() {
            Text('转换非 MP4 视频为 H.264（兼容模式）').fontSize(12).fontColor(Theme.TEXT).layoutWeight(1)
            Toggle({ type: ToggleType.Switch, isOn: this.settings.video_convert_to_h264 })
              .enabled(this.videoAvailable())
              .onChange((v: boolean) => { this.patch({ video_convert_to_h264: v }); })
          }.width('100%')

          if (this.showAdvanced) {
            Text(`图片最大宽度 ${this.settings.image_max_width_enabled ? this.settings.image_max_width : '不限'}`)
              .fontSize(12).fontColor(Theme.TEXT)
            Row() {
              Toggle({ type: ToggleType.Switch, isOn: this.settings.image_max_width_enabled })
                .onChange((v: boolean) => { this.patch({ image_max_width_enabled: v }); })
              Slider({ value: this.settings.image_max_width, min: 512, max: 4096, step: 64 })
                .layoutWeight(1)
                .onChange((v: number) => { this.patch({ image_max_width: Math.round(v) }); })
            }.width('100%')
            Row() {
              Text('GIF 调色板优化').fontSize(12).fontColor(Theme.TEXT).layoutWeight(1)
              Toggle({ type: ToggleType.Switch, isOn: this.settings.gif_palette_optimize })
                .onChange((v: boolean) => { this.patch({ gif_palette_optimize: v }); })
            }.width('100%')
          }
          Button(this.showAdvanced ? '收起高级设置' : '展开高级设置')
            .fontSize(12).backgroundColor(Theme.SURFACE).fontColor(Theme.TEXT_2)
            .onClick(() => {
              this.showAdvanced = !this.showAdvanced;
              this.saveSettings();
            })
        }
        .width('100%').padding(16).backgroundColor(Theme.WHITE)
        .border({ width: 1, color: Theme.LINE, radius: 10 })
        .alignItems(HorizontalAlign.Start)

        Column() {
          this.CardHeader('处理结果')
          Progress({ value: this.progress, total: 100, type: ProgressType.Linear })
            .color(Theme.ACCENT).width('100%')
          Text(this.appState === AppState.Compressing
            ? `${this.progress}% · ${this.progressFile}`
            : this.appState === AppState.Done ? '处理完成' : '等待开始')
            .fontSize(12).fontColor(Theme.TEXT_2)
          if (this.resultOriginal > 0) {
            Text(`${formatBytes(this.resultOriginal)} → ${formatBytes(this.resultCompressed)}`)
              .fontSize(18).fontWeight(FontWeight.Medium).fontColor(Theme.GREEN)
          }
          if (this.errorText.length > 0) {
            Text(this.errorText).fontSize(12).fontColor(Theme.RED)
          }
          if (this.hint.length > 0) {
            Text(this.hint).fontSize(12).fontColor(Theme.ACCENT_DARK)
          }
          if (this.outputPath.length > 0) {
            Button('导出结果').fontSize(12).onClick(() => { this.exportResult(); })
          }
        }
        .width('100%').padding(16).backgroundColor(Theme.WHITE)
        .border({ width: 1, color: Theme.LINE, radius: 10 })
        .alignItems(HorizontalAlign.Start)

        Column() {
          this.CardHeader('活动日志')
          if (this.activityLog.length === 0) {
            Text('暂无日志').fontSize(12).fontColor(Theme.MUTED)
          } else {
            ForEach(this.activityLog, (line: string, i: number) => {
              Text(line).fontSize(11).fontColor(Theme.TEXT_2).width('100%')
            }, (line: string, i: number) => `log-${i}`)
          }
        }
        .width('100%').padding(16).backgroundColor(Theme.WHITE)
        .border({ width: 1, color: Theme.LINE, radius: 10 })
        .alignItems(HorizontalAlign.Start)

        Column() {
          this.CardHeader('环境说明')
          Text(`内核 ${this.capsVersion()}`).fontSize(11).fontColor(Theme.MUTED)
          Text(`运行平台 ${this.capsPlatform()}`).fontSize(11).fontColor(Theme.MUTED)
          Text(this.videoAvailable()
            ? '视频压缩需要本机可用的 FFmpeg。'
            : '鸿蒙 HAP 沙箱内不提供 FFmpeg，视频保持原样；其余压缩能力完整可用。')
            .fontSize(11).fontColor(Theme.MUTED)
        }
        .width('100%').padding(16).backgroundColor(Theme.WHITE)
        .border({ width: 1, color: Theme.LINE, radius: 10 })
        .alignItems(HorizontalAlign.Start)
      }.width('100%').padding({ left: 8, right: 16, top: 12, bottom: 12 })
    }.layoutWeight(1)
  }

  build() {
    Column() {
      Row() {
        Column({ space: 2 }) {
          Text('PPTX 压缩器').fontSize(20).fontWeight(FontWeight.Bold).fontColor(Theme.TEXT)
          Text('压缩演示文稿中的图片 / GIF / 视频，保持内容不变')
            .fontSize(11).fontColor(Theme.MUTED)
        }.alignItems(HorizontalAlign.Start).layoutWeight(1)
        Text(stateLabel(this.appState, this.progress))
          .fontSize(12)
          .fontColor(stateColor(this.appState))
          .backgroundColor(stateBg(this.appState))
          .padding({ left: 12, right: 12, top: 6, bottom: 6 })
          .borderRadius(12)
      }
      .width('100%')
      .padding({ left: 20, right: 20, top: 14, bottom: 14 })

      Divider().color(Theme.LINE)

      if (this.wide) {
        Row() {
          Column() { this.LeftPane() }.layoutWeight(6)
          Column() { this.RightPane() }.layoutWeight(4)
        }.layoutWeight(1).width('100%')
      } else {
        Column() {
          this.LeftPane()
          this.RightPane()
        }.layoutWeight(1).width('100%')
      }

      Divider().color(Theme.LINE)
      Row({ space: 10 }) {
        Button('选择 PPTX')
          .fontSize(13)
          .backgroundColor(Theme.WHITE)
          .fontColor(Theme.TEXT)
          .border({ width: 1, color: Theme.LINE })
          .onClick(() => { this.pickFiles(); })
        Text(`已选 ${this.inputPaths.length} 个文件`).fontSize(12).fontColor(Theme.MUTED).layoutWeight(1)
        if (this.appState === AppState.Compressing) {
          Button('取消').fontSize(13).backgroundColor(Theme.RED_SOFT).fontColor(Theme.RED)
            .onClick(() => { this.cancelJob(); })
        } else {
          Button('开始压缩')
            .fontSize(13)
            .backgroundColor(Theme.ACCENT)
            .fontColor(Color.White)
            .enabled(this.appState === AppState.Ready && this.inputPaths.length > 0)
            .onClick(() => { this.startCompression(); })
        }
      }
      .width('100%')
      .padding({ left: 20, right: 20, top: 12, bottom: 12 })
    }
    .width('100%')
    .height('100%')
    .backgroundColor(Theme.WHITE)
    .onAreaChange((_old: Area, newValue: Area) => {
      this.wide = newValue.width >= 900;
    })
  }
}
