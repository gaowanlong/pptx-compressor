//! 鸿蒙 HAP 的 C ABI 入口。
//!
//! C++ N-API 层（`ohos/entry/src/main/cpp/napi_init.cpp`）只链接这一个符号
//! `pc_call`，用 JSON 信封完成所有操作。长度采用「两遍调用」协议，
//! 避免跨语言 malloc/free：
//!
//! 1. `pc_call(req, nullptr, 0)` 返回所需字节数；
//! 2. 分配 `len+1` 后再次调用，写入 UTF-8 + `\0`，返回实际字节数；
//! 3. 缓冲不足时返回 `-所需字节数`，内存错误返回 -1。
//!
//! 所有输入都不可信：JSON 解析失败、未知 op、空指针都返回结构化错误，
//! panic 被 `catch_unwind` 拦住，绝不跨越 FFI 边界展开。

use crate::core::{bridge, platform};
use serde::Deserialize;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr};
use std::panic::{self, AssertUnwindSafe};

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum FfiRequest {
    Version,
    Capabilities,
    /// 由 ArkTS 在 EntryAbility 启动时注入沙箱目录。
    Init {
        data_dir: String,
        cache_dir: String,
        temp_dir: String,
    },
    StartJob {
        /// 已经序列化好的 `BridgeRequest` JSON 字符串。
        request: String,
    },
    PollEvents {
        job_id: u64,
    },
    Cancel {
        job_id: u64,
    },
    DropJob {
        job_id: u64,
    },
    DropSession {
        session_id: u64,
    },
    /// 返回桌面版同源的默认设置 JSON（`CompressionSettings`）。
    DefaultSettings,
}

fn ok(data: Value) -> String {
    json!({ "ok": true, "data": data }).to_string()
}

fn err(message: impl Into<String>) -> String {
    json!({ "ok": false, "error": message.into() }).to_string()
}

/// 纯 Rust 分发逻辑，桌面端测试可直接调用。
pub fn dispatch_json(input: &str) -> String {
    let parsed: Result<FfiRequest, _> = serde_json::from_str(input);
    let request = match parsed {
        Ok(r) => r,
        Err(e) => return err(format!("请求格式错误：{e}")),
    };

    match request {
        FfiRequest::Version => ok(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "protocol_version": bridge::PROTOCOL_VERSION,
            "platform": platform::platform_label(),
        })),
        FfiRequest::Capabilities => ok(serde_json::to_value(bridge::capabilities())
            .unwrap_or_else(|e| json!({ "error": e.to_string() }))),
        FfiRequest::Init {
            data_dir,
            cache_dir,
            temp_dir,
        } => {
            let paths = platform::SandboxPaths {
                data_dir: data_dir.into(),
                cache_dir: cache_dir.into(),
                temp_dir: temp_dir.into(),
            };
            match platform::init_sandbox(paths) {
                Ok(()) => ok(serde_json::to_value(bridge::capabilities())
                    .unwrap_or_else(|e| json!({ "error": e.to_string() }))),
                Err(e) => err(e),
            }
        }
        FfiRequest::StartJob { request } => {
            match bridge::start_job(&request) {
                Ok(reply) => ok(serde_json::to_value(reply)
                    .unwrap_or_else(|e| json!({ "error": e.to_string() }))),
                Err(e) => err(e),
            }
        }
        FfiRequest::PollEvents { job_id } => ok(serde_json::to_value(bridge::poll_events(job_id))
            .unwrap_or_else(|e| json!({ "error": e.to_string() }))),
        FfiRequest::Cancel { job_id } => ok(json!({ "accepted": bridge::cancel_job(job_id) })),
        FfiRequest::DropJob { job_id } => ok(json!({ "removed": bridge::drop_job(job_id) })),
        FfiRequest::DropSession { session_id } => {
            ok(json!({ "removed": bridge::drop_session(session_id) }))
        }
        FfiRequest::DefaultSettings => ok(
            serde_json::to_value(crate::core::CompressionSettings::default())
                .unwrap_or_else(|e| json!({ "error": e.to_string() })),
        ),
    }
}

/// C ABI 版本号，供 N-API 层做加载期校验。
#[no_mangle]
pub extern "C" fn pc_abi_version() -> u32 {
    bridge::PROTOCOL_VERSION
}

/// 唯一的 JSON 调用入口。详见模块文档。
///
/// # Safety
/// `request` 必须指向合法的 NUL 结尾 UTF-8 C 字符串；`out` 为 nullptr 时
/// `out_cap` 必须为 0，否则 `out` 必须可写 `out_cap` 字节。
#[no_mangle]
pub unsafe extern "C" fn pc_call(
    request: *const c_char,
    out: *mut c_char,
    out_cap: usize,
) -> isize {
    if request.is_null() {
        return -1;
    }
    let input = match CStr::from_ptr(request).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let response = panic::catch_unwind(AssertUnwindSafe(|| dispatch_json(input)))
        .unwrap_or_else(|_| err("Rust 内部错误（已拦截 panic）"));
    let bytes = response.as_bytes();
    let needed = bytes.len() + 1;

    if out.is_null() || out_cap == 0 {
        return needed as isize;
    }
    if out_cap < needed {
        return -(needed as isize);
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), out.cast::<u8>(), bytes.len());
    *out.add(bytes.len()) = 0;
    bytes.len() as isize
}
