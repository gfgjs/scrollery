#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// Rust 只共享固定宽度字段；官方 VPL 的 packed 类型全部留在 C++ 内。
typedef struct ScrolleryVplResult {
    uint32_t width;
    uint32_t height;
    uint32_t source_width;
    uint32_t source_height;
    int32_t header_status;
    int32_t query_status;
    int32_t init_status;
    int32_t decode_status;
} ScrolleryVplResult;

typedef struct ScrolleryVplOpenResult {
    // 0=入口，1=过滤，2=枚举，3=设备匹配，4=创建会话，5=绑定设备，6=可用。
    uint32_t stage;
    int32_t status;
} ScrolleryVplOpenResult;

// 返回线程独占会话；不存在精确匹配的硬件 runtime 时返回空。
void* scrollery_vpl_create(uint32_t luid_low, int32_t luid_high,
                          uint32_t vendor_id, uint32_t device_id, ScrolleryVplOpenResult* result);
void scrollery_vpl_destroy(void* session);

// 0=完整硬件路径成功，1=不适用/不支持，2=部分加速，3=执行失败。
// 输入为受控文件读出的完整 JPEG；输出为调用方提供的有界 RGBA 小图缓冲。
// ICC 与 EXIF 由调用方保留/应用；本接口不接路径，不写文件。
int32_t scrollery_vpl_decode(void* session, uint8_t* jpeg, uint32_t jpeg_bytes,
                            uint32_t long_edge, uint8_t* rgba,
                            uint32_t rgba_capacity, ScrolleryVplResult* result);

#ifdef __cplusplus
}
#endif
