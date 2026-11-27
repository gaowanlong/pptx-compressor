import { preferences } from '@kit.ArkData';
import {
  DEFAULT_STATE,
  PersistedState,
  STORAGE_KEY,
} from './Protocol';
import { NativeBridge } from './Bridge';

const STORE_NAME: string = 'pptx_compressor_prefs';

/**
 * 偏好存储。键名与 JSON 结构刻意与桌面版 `core::state` 完全一致，
 * 因此用户在 Windows / macOS 上的设置可以直接迁移到鸿蒙电脑。
 */
export class SettingsStore {
  private static store: preferences.Preferences | null = null;

  static async init(context: Context): Promise<void> {
    SettingsStore.store = await preferences.getPreferences(context, STORE_NAME);
  }

  static async load(): Promise<PersistedState> {
    const store = SettingsStore.store;
    if (store === null) {
      return DEFAULT_STATE;
    }
    const raw = await store.get(STORAGE_KEY, '') as string;
    if (raw.length === 0) {
      // 首次启动：向 Rust 内核要一份权威默认值，避免两端默认值漂移。
      const reply = NativeBridge.defaultSettings();
      const state: PersistedState = {
        settings: reply.ok && reply.data !== undefined ? reply.data : DEFAULT_STATE.settings,
        output_dir: null,
        show_advanced: false,
        ffmpeg_override: null,
      };
      return state;
    }
    try {
      return JSON.parse(raw) as PersistedState;
    } catch (err) {
      return DEFAULT_STATE;
    }
  }

  static async save(state: PersistedState): Promise<void> {
    const store = SettingsStore.store;
    if (store === null) {
      return;
    }
    await store.put(STORAGE_KEY, JSON.stringify(state));
    await store.flush();
  }
}
