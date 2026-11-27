#![cfg(feature = "ffi")]
//! C ABI 契约测试：在主机上验证 N-API 层将要使用的两遍长度协议。

use pptx_compressor::core::bridge::PROTOCOL_VERSION;
use pptx_compressor::ffi::{dispatch_json, pc_abi_version, pc_call};
use std::ffi::{c_char, CStr, CString};

fn call_c(request: &str) -> String {
    let input = CString::new(request).unwrap();
    let needed = unsafe { pc_call(input.as_ptr(), std::ptr::null_mut(), 0) };
    assert!(needed > 0, "首遍调用应返回所需字节数");
    let mut buffer: Vec<c_char> = vec![0; needed as usize];
    let written = unsafe { pc_call(input.as_ptr(), buffer.as_mut_ptr(), buffer.len()) };
    assert!(written >= 0, "第二遍调用应成功");
    assert_eq!(written as usize + 1, buffer.len());
    let text = unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_str().unwrap();
    assert_eq!(text.len(), written as usize);
    text.to_owned()
}

#[test]
fn abi_version_matches_protocol() {
    assert_eq!(pc_abi_version(), PROTOCOL_VERSION);
}

#[test]
fn two_pass_protocol_returns_structured_json() {
    let version = call_c(r#"{"op":"version"}"#);
    let value: serde_json::Value = serde_json::from_str(&version).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["data"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(value["data"]["protocol_version"], PROTOCOL_VERSION);

    let caps = call_c(r#"{"op":"capabilities"}"#);
    let value: serde_json::Value = serde_json::from_str(&caps).unwrap();
    assert_eq!(value["ok"], true);
    assert!(value["data"]["platform"].is_string());

    let settings = call_c(r#"{"op":"default_settings"}"#);
    let value: serde_json::Value = serde_json::from_str(&settings).unwrap();
    assert_eq!(value["data"]["image_quality"], 75);
    assert_eq!(value["data"]["video_quality"], 45);
}

#[test]
fn malformed_requests_never_panic_across_ffi() {
    let bad = call_c("not-json");
    let value: serde_json::Value = serde_json::from_str(&bad).unwrap();
    assert_eq!(value["ok"], false);
    assert!(value["error"].is_string());

    let unknown = dispatch_json(r#"{"op":"does_not_exist"}"#);
    let value: serde_json::Value = serde_json::from_str(&unknown).unwrap();
    assert_eq!(value["ok"], false);

    let input = CString::new(r#"{"op":"version"}"#).unwrap();
    assert_eq!(unsafe { pc_call(std::ptr::null(), std::ptr::null_mut(), 0) }, -1);
    let mut tiny: Vec<c_char> = vec![0; 2];
    let short = unsafe { pc_call(input.as_ptr(), tiny.as_mut_ptr(), tiny.len()) };
    assert!(short < 0, "缓冲区不足应返回负的所需长度");
}

#[test]
fn init_reports_capabilities_and_settings_schema() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("files");
    let cache = dir.path().join("cache");
    let temp = dir.path().join("cache/tmp");
    let request = serde_json::json!({
        "op": "init",
        "data_dir": data,
        "cache_dir": cache,
        "temp_dir": temp,
    });
    let reply = dispatch_json(&request.to_string());
    let value: serde_json::Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(value["ok"], true);
    assert!(temp.exists(), "init 必须创建临时目录");
    assert_eq!(value["data"]["protocol_version"], PROTOCOL_VERSION);
}
