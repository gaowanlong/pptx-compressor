#include <napi/native_api.h>
#include <hilog/log.h>

#include <cstdint>
#include <cstring>
#include <string>
#include <vector>

#include "pptx_bridge.h"

#undef LOG_DOMAIN
#undef LOG_TAG
#define LOG_DOMAIN 0x5058
#define LOG_TAG "PptxBridge"

namespace {

constexpr const char *kFallback =
    "{\"ok\":false,\"error\":\"桥接层不可用：Rust 静态库未正确链接\"}";

std::string ToUtf8(napi_env env, napi_value value) {
    size_t len = 0;
    if (napi_get_value_string_utf8(env, value, nullptr, 0, &len) != napi_ok) {
        return "";
    }
    std::string result(len + 1, '\0');
    size_t written = 0;
    if (napi_get_value_string_utf8(env, value, &result[0], result.size(), &written) != napi_ok) {
        return "";
    }
    result.resize(written);
    return result;
}

napi_value MakeString(napi_env env, const std::string &text) {
    napi_value out = nullptr;
    napi_create_string_utf8(env, text.c_str(), text.size(), &out);
    return out;
}

/* 两遍长度协议：先问长度，再取内容；缓冲不足时按返回值扩容重试。 */
std::string InvokeRust(const std::string &request) {
    if (pc_abi_version() != 1U) {
        return std::string(kFallback);
    }
    intptr_t needed = pc_call(request.c_str(), nullptr, 0);
    if (needed <= 0) {
        return std::string(kFallback);
    }

    std::vector<char> buffer(static_cast<size_t>(needed) + 1, '\0');
    for (int attempt = 0; attempt < 3; ++attempt) {
        intptr_t written = pc_call(request.c_str(), buffer.data(), buffer.size());
        if (written >= 0) {
            return std::string(buffer.data(), static_cast<size_t>(written));
        }
        size_t want = static_cast<size_t>(-written);
        if (want <= buffer.size()) {
            break;
        }
        buffer.assign(want + 1, '\0');
    }
    return std::string(kFallback);
}

napi_value Call(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1] = {nullptr};
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);
    if (argc < 1 || args[0] == nullptr) {
        return MakeString(env, "{\"ok\":false,\"error\":\"缺少请求字符串\"}");
    }
    std::string request = ToUtf8(env, args[0]);
    if (request.empty()) {
        return MakeString(env, "{\"ok\":false,\"error\":\"请求字符串为空\"}");
    }
    return MakeString(env, InvokeRust(request));
}

napi_value AbiVersion(napi_env env, napi_callback_info info) {
    (void)info;
    napi_value out = nullptr;
    napi_create_uint32(env, pc_abi_version(), &out);
    return out;
}

napi_value Init(napi_env env, napi_value exports) {
    napi_property_descriptor desc[] = {
        {"call", nullptr, Call, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"abiVersion", nullptr, AbiVersion, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    napi_define_properties(env, exports, sizeof(desc) / sizeof(desc[0]), desc);
    OH_LOG_INFO(LOG_APP, "PPTX bridge loaded, abi=%{public}u", pc_abi_version());
    return exports;
}

}  // namespace

extern "C" __attribute__((constructor)) void RegisterPptxModule(void) {
    static napi_module module = {
        .nm_version = 1,
        .nm_flags = 0,
        .nm_filename = nullptr,
        .nm_register_func = Init,
        .nm_modname = "entry",
        .nm_priv = nullptr,
        .reserved = {nullptr},
    };
    napi_module_register(&module);
}
