/**
 * 与 Rust 侧 `core::bridge` 一一对应的协议模型。
 * 字段名、枚举值、可空语义必须与 serde 输出严格一致，任一改动都要
 * 同步修改 Rust 的 struct / #[serde(rename_all)]。
 */

export interface CompressionSettings {
  image_quality: number;
  video_quality: number;
  gif_palette_optimize: boolean;
  gif_frame_drop: boolean;
  gif_target_fps: number;
  image_max_width_enabled: boolean;
  image_max_width: number;
  video_max_resolution_enabled: boolean;
  video_max_height: number;
  video_convert_to_h264: boolean;
}

/** 持久化结构与桌面版 `core::state::PersistedState` 完全一致。 */
export interface PersistedState {
  settings: CompressionSettings;
  output_dir: string | null;
  show_advanced: boolean;
  ffmpeg_override: string | null;
}

export interface Capabilities {
  platform: string;
  version: string;
  protocol_version: number;
  video_transcode: boolean;
  batch: boolean;
  max_image_threads: number;
}

export interface StartReply {
  job_id: number;
  capabilities: Capabilities;
}

export interface TypeBytes {
  count: number;
  bytes: number;
}

export interface MediaComposition {
  package_size: number;
  media_bytes: number;
  optimizable_bytes: number;
  optimizable_count: number;
  total_count: number;
  jpeg: TypeBytes;
  png: TypeBytes;
  gif: TypeBytes;
  video: TypeBytes;
  other: TypeBytes;
}

export interface BridgeMedia {
  index: number;
  path: string;
  name: string;
  kind: string; // jpeg | png | gif | video | other
  original_size: number;
  enabled: boolean;
  status: string; // pending | processing | done | skipped | failed
}

/**
 * 事件采用 `type` 标签化枚举；不同事件只填自己的字段，其余为 undefined。
 * 用单接口 + 可选字段是为了遵守 ArkTS 的静态类型约束。
 */
export interface BridgeEvent {
  type: string;
  session_id?: number;
  source_path?: string;
  source_size?: number;
  warnings?: string[];
  media?: BridgeMedia[];
  composition?: MediaComposition;
  index?: number;
  status?: string;
  compressed_size?: number;
  name?: string;
  media_count?: number;
  current?: number;
  total?: number;
  file?: string;
  message?: string;
  original?: number;
  compressed?: number;
  output_path?: string;
  summary?: BatchSummary;
}

export interface BatchSummary {
  total: number;
  succeeded: number;
  failed: number;
  total_original: number;
  total_compressed: number;
}

export interface PollReply {
  events: BridgeEvent[];
  done: boolean;
}

/** N-API 调用信封。所有对象字面量必须有显式类型（ArkTS 要求）。 */
export interface BridgeCall {
  op: string;
  request?: string;
  job_id?: number;
  session_id?: number;
  data_dir?: string;
  cache_dir?: string;
  temp_dir?: string;
}

/** 局部修改设置；ArkTS 不允许对象展开，用显式 patch + ?? 合并。 */
export interface SettingsPatch {
  image_quality?: number;
  video_quality?: number;
  gif_palette_optimize?: boolean;
  gif_frame_drop?: boolean;
  gif_target_fps?: number;
  image_max_width_enabled?: boolean;
  image_max_width?: number;
  video_max_resolution_enabled?: boolean;
  video_max_height?: number;
  video_convert_to_h264?: boolean;
}

export interface BridgeResponse<T> {
  ok: boolean;
  data?: T;
  error?: string;
}

/** 无返回体的操作（cancel / drop_*）。 */
export interface EmptyData {
  accepted?: boolean;
  removed?: boolean;
}

export interface InspectRequest {
  mode: string;
  path: string;
}

export interface CompressRequest {
  mode: string;
  session_id: number;
  output_dir: string | null;
  settings: CompressionSettings;
  enabled: boolean[];
}

export interface BatchRequest {
  mode: string;
  inputs: string[];
  output_dir: string | null;
  settings: CompressionSettings;
}

/** 与 Rust `CompressionSettings::default()` 保持同步的兜底值。 */
export const DEFAULT_SETTINGS: CompressionSettings = {
  image_quality: 75,
  video_quality: 45,
  gif_palette_optimize: true,
  gif_frame_drop: true,
  gif_target_fps: 15,
  image_max_width_enabled: false,
  image_max_width: 1920,
  video_max_resolution_enabled: false,
  video_max_height: 1080,
  video_convert_to_h264: true,
};

export const DEFAULT_STATE: PersistedState = {
  settings: DEFAULT_SETTINGS,
  output_dir: null,
  show_advanced: false,
  ffmpeg_override: null,
};

export const PROTOCOL_VERSION: number = 1;
export const STORAGE_KEY: string = 'pptx-compressor.state.v1';
