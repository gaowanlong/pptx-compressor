cmake_minimum_required(VERSION 3.16)

project(pptx_compressor_napi LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

set(NATIVE_ROOT ${CMAKE_CURRENT_SOURCE_DIR})
include_directories(${NATIVE_ROOT} ${NATIVE_ROOT}/include)

# OHOS_ARCH 由 hvigor 注入：arm64-v8a（真机）/ x86_64（PC 模拟器）。
set(RUST_STATIC_LIB "${NATIVE_ROOT}/libs/${OHOS_ARCH}/libpptx_compressor.a")
if(NOT EXISTS ${RUST_STATIC_LIB})
    message(FATAL_ERROR
        "缺少 Rust 静态库: ${RUST_STATIC_LIB}\n"
        "请先在仓库根目录执行: ./scripts/build-ohos.sh")
endif()

add_library(entry SHARED napi_init.cpp)

target_link_libraries(entry PUBLIC
    ${RUST_STATIC_LIB}
    libace_napi.z.so
    libhilog_ndk.z.so
    libuv.so
    z
)

# 与桌面版一致的发布裁剪：去掉未引用符号，控制 HAP 体积。
target_compile_options(entry PRIVATE -fvisibility=hidden -ffunction-sections -fdata-sections)
target_link_options(entry PRIVATE -Wl,--gc-sections)
