#include <chrono>
#include "bridge.h"
#include <vpl/mfxdispatcher.h>
#include <vpl/mfxjpeg.h>
#include <vpl/mfxvideo.h>
#include <d3d11.h>
#include <d3d10.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <algorithm>
#include <cstring>
#include <memory>
#include <new>

namespace {
using Microsoft::WRL::ComPtr;
constexpr int32_t unavailable = 1, partial = 2, failed = 3;
constexpr int32_t not_called = INT32_MIN;

struct Session {
    mfxLoader loader = nullptr;
    mfxSession session = nullptr;
    bool healthy = true;
    ComPtr<ID3D11Device> device;
    ~Session() {
        if (session) MFXClose(session);
        if (loader) MFXUnload(loader);
    }
};

bool filter(mfxLoader loader, const char* name, mfxU32 value) {
    const auto config = MFXCreateConfig(loader);
    if (!config) return false;
    mfxVariant variant{};
    variant.Type = MFX_VARIANT_TYPE_U32;
    variant.Data.U32 = value;
    return MFXSetConfigFilterProperty(config,
        reinterpret_cast<const mfxU8*>(name), variant) == MFX_ERR_NONE;
}

bool bind_device(Session& owned, const LUID& luid, uint32_t vendor, uint32_t device, int32_t& status) {
    ComPtr<IDXGIFactory1> factory;
    status = CreateDXGIFactory1(IID_PPV_ARGS(&factory));
    if (FAILED(status)) return false;
    for (UINT index = 0; ; ++index) {
        ComPtr<IDXGIAdapter1> adapter;
        if (factory->EnumAdapters1(index, &adapter) == DXGI_ERROR_NOT_FOUND) return false;
        DXGI_ADAPTER_DESC1 desc{};
        if (!adapter || FAILED(adapter->GetDesc1(&desc))) return false;
        if (desc.AdapterLuid.LowPart != luid.LowPart || desc.AdapterLuid.HighPart != luid.HighPart ||
            desc.VendorId != vendor || desc.DeviceId != device) continue;
        status = D3D11CreateDevice(adapter.Get(), D3D_DRIVER_TYPE_UNKNOWN, nullptr,
            D3D11_CREATE_DEVICE_VIDEO_SUPPORT | D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            nullptr, 0, D3D11_SDK_VERSION, &owned.device, nullptr, nullptr);
        if (FAILED(status)) return false;
        ComPtr<ID3D10Multithread> multithread;
        status = owned.device.As(&multithread);
        if (FAILED(status)) return false;
        multithread->SetMultithreadProtected(TRUE);
        // runtime 尚未创建内部设备时显式绑定；不能在 Init 前假定 GetHandle 已有对象。
        status = MFXVideoCORE_SetHandle(owned.session, MFX_HANDLE_D3D11_DEVICE, owned.device.Get());
        return status == MFX_ERR_NONE;
    }
}

struct Pipeline {
    mfxSession session;
    ~Pipeline() { MFXVideoVPP_Close(session); MFXVideoDECODE_Close(session); }
};

struct Surface {
    mfxFrameSurface1* frame = nullptr;
    ~Surface() { if (frame) frame->FrameInterface->Release(frame); }
};

int32_t classify(mfxStatus status) {
    if (status == MFX_WRN_PARTIAL_ACCELERATION) return partial;
    if (status == MFX_ERR_UNSUPPORTED || status == MFX_ERR_INVALID_VIDEO_PARAM ||
        status > 0) return unavailable;
    return failed;
}
} // namespace

extern "C" void* scrollery_vpl_create(uint32_t low, int32_t high, uint32_t vendor, uint32_t device,
                                     ScrolleryVplOpenResult* report) {
    ScrolleryVplOpenResult local{};
    if (!report) report = &local;
    *report = {};
    // dispatcher 限定硬件、JPEG、D3D11 和内部 surface 解码/缩放所需 API，缺 runtime 正常返回空。
    try {
        auto owned = std::make_unique<Session>();
        owned->loader = MFXLoad();
        report->stage = 1;
        if (!owned->loader ||
            !filter(owned->loader, "mfxImplDescription.Impl", MFX_IMPL_TYPE_HARDWARE) ||
            !filter(owned->loader, "mfxImplDescription.AccelerationMode", MFX_ACCEL_MODE_VIA_D3D11) ||
            !filter(owned->loader, "mfxImplDescription.ApiVersion.Version", (2u << 16) | 1u) ||
            !filter(owned->loader, "mfxImplDescription.mfxDecoderDescription.decoder.CodecID", MFX_CODEC_JPEG))
            return nullptr;
        const LUID luid{low, high};
        for (mfxU32 index = 0; index < 64; ++index) {
            mfxHDL caps = nullptr;
            const auto status = MFXEnumImplementations(owned->loader, index, MFX_IMPLCAPS_DEVICE_ID_EXTENDED, &caps);
            report->stage = 2;
            report->status = status;
            if (status == MFX_ERR_NOT_FOUND) break;
            if (status != MFX_ERR_NONE || !caps) continue;
            const auto* id = static_cast<const mfxExtendedDeviceId*>(caps);
            report->stage = 3;
            // 首版只接受单节点设备，不能把 linked adapter 的其它节点误记为本设备。
            const bool matches = id->LUIDValid && id->LUIDDeviceNodeMask == 1 &&
                id->VendorID == vendor && id->DeviceID == device &&
                std::memcmp(id->DeviceLUID, &luid, sizeof(luid)) == 0;
            MFXDispReleaseImplDescription(owned->loader, caps);
            if (!matches) continue;
            report->stage = 4;
            report->status = MFXCreateSession(owned->loader, index, &owned->session);
            if (report->status == MFX_ERR_NONE) {
                report->stage = 5;
                if (bind_device(*owned, luid, vendor, device, report->status)) {
                    report->stage = 6;
                    return owned.release();
                }
            }
            if (owned->session) MFXClose(owned->session);
            owned->session = nullptr;
            owned->device.Reset();
            return nullptr;
        }
    } catch (...) {
        // C++ 异常不得越过 Rust 的 C ABI。
    }
    return nullptr;
}

extern "C" int32_t scrollery_vpl_healthy(void* session) {
    return session && static_cast<Session*>(session)->healthy ? 1 : 0;
}

extern "C" void scrollery_vpl_destroy(void* session) {
    delete static_cast<Session*>(session);
}

extern "C" int32_t scrollery_vpl_decode(void* opaque, uint8_t* jpeg, uint32_t length,
                                       uint32_t edge, uint8_t* rgba, uint32_t capacity,
                                       ScrolleryVplResult* result) {
    if (!result) return failed;
    *result = {0, 0, 0, 0, not_called, not_called, not_called, not_called, not_called, 0, 0, 0, 0};
    const auto timed = [](uint64_t& elapsed, auto&& operation) {
        const auto start = std::chrono::steady_clock::now();
        const auto status = operation();
        elapsed += std::max<int64_t>(1, std::chrono::duration_cast<std::chrono::microseconds>(std::chrono::steady_clock::now() - start).count());
        return status;
    };
    if (!opaque || !jpeg || !rgba || length < 32 || length > 32u * 1024 * 1024 ||
        edge == 0 || edge > 2048) return unavailable;
    auto& owned = *static_cast<Session*>(opaque);
    if (!owned.healthy) return unavailable;
    struct HealthGuard {
        Session& session;
        ScrolleryVplResult& result;
        ~HealthGuard() {
            for (auto status : {result.header_status, result.query_status, result.init_status, result.decode_status}) {
                if (status == MFX_ERR_DEVICE_LOST || status == MFX_ERR_DEVICE_FAILED) session.healthy = false;
            }
        }
    } health{owned, *result};
    try {
        mfxBitstream bits{};
        bits.Data = jpeg;
        bits.DataLength = bits.MaxLength = length;
        bits.CodecId = MFX_CODEC_JPEG;
        bits.DataFlag = MFX_BITSTREAM_COMPLETE_FRAME;
        mfxVideoParam params{};
        params.mfx.CodecId = MFX_CODEC_JPEG;
        auto status = MFXVideoDECODE_DecodeHeader(owned.session, &bits, &params);
        result->header_status = status;
        if (status != MFX_ERR_NONE) return classify(status);
        const auto width = params.mfx.FrameInfo.CropW;
        const auto height = params.mfx.FrameInfo.CropH;
        result->source_width = width;
        result->source_height = height;
        if ((params.mfx.CodecProfile != 0 && params.mfx.CodecProfile != MFX_PROFILE_JPEG_BASELINE) || !width || !height ||
            width > 8192 || height > 8192 || uint64_t(width) * height * 4 > 64u * 1024 * 1024 ||
            (params.mfx.FrameInfo.PicStruct != MFX_PICSTRUCT_UNKNOWN &&
             params.mfx.FrameInfo.PicStruct != MFX_PICSTRUCT_PROGRESSIVE))
            return unavailable;
        if (params.mfx.JPEGColorFormat != MFX_JPEG_COLORFORMAT_YCbCr &&
            params.mfx.JPEGColorFormat != MFX_JPEG_COLORFORMAT_UNKNOWN) return unavailable;
        // NV12 路径不额外降低 4:2:2/4:4:4 色度分辨率，这些源留给现有正确 CPU 路径。
        if (params.mfx.JPEGChromaFormat != MFX_CHROMAFORMAT_YUV420) return unavailable;
        const auto longest = std::max(width, height);
        result->width = longest > edge ? std::max(1u, uint32_t(width) * edge / longest) : width;
        result->height = longest > edge ? std::max(1u, uint32_t(height) * edge / longest) : height;
        if (uint64_t(result->width) * result->height * 4 > capacity) return unavailable;

        // JPEG 使用全范围 BT.601；显式交给 VPP，避免 runtime 默认按视频有限范围转 RGB。
        params.IOPattern = MFX_IOPATTERN_OUT_VIDEO_MEMORY;
        params.AsyncDepth = 1;
        params.mfx.CodecProfile = MFX_PROFILE_JPEG_BASELINE;
        params.mfx.FrameInfo.PicStruct = MFX_PICSTRUCT_PROGRESSIVE;
        params.mfx.FrameInfo.FourCC = MFX_FOURCC_NV12;
        params.mfx.FrameInfo.ChromaFormat = MFX_CHROMAFORMAT_YUV420;
        params.mfx.FrameInfo.FrameRateExtN = 30;
        params.mfx.FrameInfo.FrameRateExtD = 1;
        mfxVideoParam queried = params;
        status = MFXVideoDECODE_Query(owned.session, &params, &queried);
        result->query_status = status;
        if (status != MFX_ERR_NONE) return classify(status);
        if (queried.mfx.FrameInfo.FourCC != MFX_FOURCC_NV12 ||
            queried.mfx.FrameInfo.CropW != width || queried.mfx.FrameInfo.CropH != height)
            return unavailable;

        mfxExtVPPScaling scaling{};
        scaling.Header = {MFX_EXTBUFF_VPP_SCALING, sizeof(scaling)};
        scaling.ScalingMode = MFX_SCALING_MODE_QUALITY;
        mfxExtVPPVideoSignalInfo color{};
        color.Header = {MFX_EXTBUFF_VPP_VIDEO_SIGNAL_INFO, sizeof(color)};
        color.In.TransferMatrix = color.Out.TransferMatrix = MFX_TRANSFERMATRIX_BT601;
        color.In.NominalRange = color.Out.NominalRange = MFX_NOMINALRANGE_0_255;
        mfxExtBuffer* extensions[] = {&scaling.Header, &color.Header};
        mfxVideoParam vpp{};
        vpp.AsyncDepth = 1;
        vpp.vpp.In = params.mfx.FrameInfo;
        vpp.vpp.Out = vpp.vpp.In;
        vpp.vpp.Out.FourCC = MFX_FOURCC_RGB4;
        vpp.vpp.Out.ChromaFormat = MFX_CHROMAFORMAT_YUV444;
        vpp.vpp.Out.CropX = vpp.vpp.Out.CropY = 0;
        vpp.vpp.Out.CropW = static_cast<mfxU16>(result->width);
        vpp.vpp.Out.CropH = static_cast<mfxU16>(result->height);
        vpp.vpp.Out.Width = (vpp.vpp.Out.CropW + 15) & ~15;
        vpp.vpp.Out.Height = (vpp.vpp.Out.CropH + 15) & ~15;
        vpp.IOPattern = MFX_IOPATTERN_IN_VIDEO_MEMORY | MFX_IOPATTERN_OUT_VIDEO_MEMORY;
        vpp.ExtParam = extensions;
        vpp.NumExtParam = 2;
        mfxVideoParam vpp_query = vpp;
        status = MFXVideoVPP_Query(owned.session, &vpp, &vpp_query);
        result->query_status = status;
        if (status != MFX_ERR_NONE) return classify(status);
        Pipeline pipeline{owned.session};
        status = timed(result->init_us, [&] { return MFXVideoDECODE_Init(owned.session, &params); });
        result->init_status = status;
        if (status != MFX_ERR_NONE) return classify(status);
        status = timed(result->init_us, [&] { return MFXVideoVPP_Init(owned.session, &vpp); });
        result->init_status = status;
        if (status != MFX_ERR_NONE) return classify(status);
        bits.DataOffset = 0;
        bits.DataLength = length;
        Surface decoded;
        mfxSyncPoint sync = nullptr;
        status = timed(result->decode_us, [&] { return MFXVideoDECODE_DecodeFrameAsync(owned.session, &bits, nullptr, &decoded.frame, &sync); });
        if (status == MFX_ERR_MORE_DATA && !decoded.frame)
            status = timed(result->decode_us, [&] { return MFXVideoDECODE_DecodeFrameAsync(owned.session, nullptr, nullptr, &decoded.frame, &sync); });
        result->decode_status = status;
        if (status != MFX_ERR_NONE || !decoded.frame) {
            if (status == MFX_ERR_DEVICE_LOST || status == MFX_ERR_DEVICE_FAILED) owned.healthy = false;
            return classify(status);
        }
        Surface output;
        status = timed(result->vpp_us, [&] { return MFXVideoVPP_ProcessFrameAsync(owned.session, decoded.frame, &output.frame); });
        result->decode_status = status;
        if (status != MFX_ERR_NONE || !output.frame) {
            if (status == MFX_ERR_DEVICE_LOST || status == MFX_ERR_DEVICE_FAILED) owned.healthy = false;
            return classify(status);
        }
        {
            auto* frame = output.frame;
            // 仅目标通道可 Map；原尺寸解码面全程留在设备内。
            if (frame->Info.FourCC != MFX_FOURCC_RGB4 || frame->Info.CropX || frame->Info.CropY ||
                frame->Info.CropW != result->width || frame->Info.CropH != result->height) return failed;
            status = timed(result->sync_us, [&] { return frame->FrameInterface->Synchronize(frame, 1500); });
            result->decode_status = status;
            result->sync_status = status;
            if (status != MFX_ERR_NONE) { owned.healthy = false; return classify(status); }
            status = frame->FrameInterface->Map(frame, MFX_MAP_READ);
            result->decode_status = status;
            if (status != MFX_ERR_NONE) return classify(status);
            const uint32_t pitch = (uint32_t(frame->Data.PitchHigh) << 16) | frame->Data.PitchLow;
            const bool valid = frame->Data.B && pitch >= result->width * 4;
            if (valid) {
                for (uint32_t y = 0; y < result->height; ++y) {
                    const auto* source = frame->Data.B + size_t(y) * pitch;
                    auto* dest = rgba + size_t(y) * result->width * 4;
                    for (uint32_t x = 0; x < result->width; ++x) {
                        dest[x * 4] = source[x * 4 + 2];
                        dest[x * 4 + 1] = source[x * 4 + 1];
                        dest[x * 4 + 2] = source[x * 4];
                        dest[x * 4 + 3] = 255;
                    }
                }
            }
            status = frame->FrameInterface->Unmap(frame);
            result->decode_status = status;
            return valid && status == MFX_ERR_NONE ? 0 : failed;
        }
    } catch (...) {
        owned.healthy = false;
    }
    return failed;
}
