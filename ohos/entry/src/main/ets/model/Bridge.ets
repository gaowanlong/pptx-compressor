import native from 'libentry.so';
import {
  BridgeCall,
  BridgeResponse,
  Capabilities,
  CompressionSettings,
  EmptyData,
  PollReply,
  StartReply,
  PROTOCOL_VERSION,
} from './Protocol';

/** N-API 薄封装：所有调用都是同步且非阻塞的，重活由 Rust 后台线程承担。 */
export class NativeBridge {
  static abiVersion(): number {
    return native.abiVersion();
  }

  private static call<T>(request: BridgeCall): BridgeResponse<T> {
    const raw: string = native.call(JSON.stringify(request));
    return JSON.parse(raw) as BridgeResponse<T>;
  }

  /** 启动期校验协议版本，避免 HAP 与静态库版本错配后静默错解字段。 */
  static handshake(): string {
    if (NativeBridge.abiVersion() !== PROTOCOL_VERSION) {
      return '内核协议版本不匹配，请重新构建 Rust 静态库';
    }
    const op: BridgeCall = { op: 'capabilities' };
    const reply: BridgeResponse<Capabilities> = NativeBridge.call<Capabilities>(op);
    if (!reply.ok || reply.data === undefined) {
      return reply.error ?? '内核能力查询失败';
    }
    if (reply.data.protocol_version !== PROTOCOL_VERSION) {
      return `内核协议版本 ${reply.data.protocol_version}，界面期望 ${PROTOCOL_VERSION}`;
    }
    return '';
  }

  static capabilities(): BridgeResponse<Capabilities> {
    const op: BridgeCall = { op: 'capabilities' };
    return NativeBridge.call<Capabilities>(op);
  }

  /** 注入沙箱目录；鸿蒙侧会据此关闭视频转码并固定临时目录。 */
  static init(dataDir: string, cacheDir: string, tempDir: string): BridgeResponse<Capabilities> {
    const op: BridgeCall = {
      op: 'init',
      data_dir: dataDir,
      cache_dir: cacheDir,
      temp_dir: tempDir,
    };
    return NativeBridge.call<Capabilities>(op);
  }

  static defaultSettings(): BridgeResponse<CompressionSettings> {
    const op: BridgeCall = { op: 'default_settings' };
    return NativeBridge.call<CompressionSettings>(op);
  }

  /** `request` 是 InspectRequest / CompressRequest / BatchRequest 的 JSON 字符串。 */
  static startJob(requestJson: string): BridgeResponse<StartReply> {
    const op: BridgeCall = { op: 'start_job', request: requestJson };
    return NativeBridge.call<StartReply>(op);
  }

  static pollEvents(jobId: number): BridgeResponse<PollReply> {
    const op: BridgeCall = { op: 'poll_events', job_id: jobId };
    return NativeBridge.call<PollReply>(op);
  }

  static cancel(jobId: number): void {
    const op: BridgeCall = { op: 'cancel', job_id: jobId };
    NativeBridge.call<EmptyData>(op);
  }

  static dropJob(jobId: number): void {
    const op: BridgeCall = { op: 'drop_job', job_id: jobId };
    NativeBridge.call<EmptyData>(op);
  }

  static dropSession(sessionId: number): void {
    const op: BridgeCall = { op: 'drop_session', session_id: sessionId };
    NativeBridge.call<EmptyData>(op);
  }
}
