#ifndef PPTX_COMPRESSOR_BRIDGE_H
#define PPTX_COMPRESSOR_BRIDGE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Rust 侧导出的协议版本，加载期校验。 */
uint32_t pc_abi_version(void);

/*
 * 统一的 JSON 调用入口。
 *   out == nullptr 或 out_cap == 0 : 返回所需缓冲区字节数（含结尾 \0）
 *   缓冲区不足                     : 返回 -所需字节数
 *   成功                           : 返回写入的字节数（不含 \0）
 *   request 非法                   : 返回 -1
 */
intptr_t pc_call(const char *request, char *out, size_t out_cap);

#ifdef __cplusplus
}
#endif

#endif /* PPTX_COMPRESSOR_BRIDGE_H */
