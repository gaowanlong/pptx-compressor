/** 与桌面版 egui 设计令牌一一对应（纯白底、单一强调色、细分隔线）。 */
export class Theme {
  static readonly WHITE: string = '#FFFFFFFF';
  static readonly SURFACE: string = '#FFF7F8FA';
  static readonly SURFACE_2: string = '#FFFBFCFD';
  static readonly LINE: string = '#FFE7E9EE';
  static readonly TEXT: string = '#FF1B1D21';
  static readonly TEXT_2: string = '#FF5B6069';
  static readonly MUTED: string = '#FF8A8F98';
  static readonly ACCENT: string = '#FF2F6BFF';
  static readonly ACCENT_DARK: string = '#FF2255DE';
  static readonly ACCENT_SOFT: string = '#FFEEF3FF';
  static readonly GREEN: string = '#FF12A150';
  static readonly GREEN_SOFT: string = '#FFEAF7EF';
  static readonly AMBER: string = '#FFC87F0A';
  static readonly RED: string = '#FFD64545';
  static readonly RED_SOFT: string = '#FFFDEEEE';
}

/** 与桌面版 `AppState` 六态一致。 */
export enum AppState {
  Idle = 'idle',
  Loading = 'loading',
  Ready = 'ready',
  Compressing = 'compressing',
  Done = 'done',
  Error = 'error',
}

export function stateLabel(state: AppState, progress: number): string {
  switch (state) {
    case AppState.Idle: return '等待文件';
    case AppState.Loading: return '正在解析';
    case AppState.Ready: return '已就绪';
    case AppState.Compressing: return `压缩中 ${progress}%`;
    case AppState.Done: return '已完成';
    case AppState.Error: return '处理失败';
    default: return '等待文件';
  }
}

export function stateColor(state: AppState): string {
  switch (state) {
    case AppState.Ready:
    case AppState.Done: return Theme.GREEN;
    case AppState.Loading:
    case AppState.Compressing: return Theme.ACCENT;
    case AppState.Error: return Theme.RED;
    default: return Theme.MUTED;
  }
}

export function stateBg(state: AppState): string {
  switch (state) {
    case AppState.Ready:
    case AppState.Done: return Theme.GREEN_SOFT;
    case AppState.Loading:
    case AppState.Compressing: return Theme.ACCENT_SOFT;
    case AppState.Error: return Theme.RED_SOFT;
    default: return Theme.SURFACE;
  }
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

export function kindLabel(kind: string): string {
  switch (kind) {
    case 'jpeg': return 'JPEG 图片';
    case 'png': return 'PNG 图片';
    case 'gif': return 'GIF 动图';
    case 'video': return '视频';
    default: return '其他';
  }
}

export function statusLabel(status: string): string {
  switch (status) {
    case 'pending': return '待处理';
    case 'processing': return '处理中';
    case 'done': return '已压缩';
    case 'skipped': return '已跳过';
    case 'failed': return '失败';
    default: return '待处理';
  }
}
