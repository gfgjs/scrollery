#include "bridge.h"
#include <windows.h>
#include <dxgi1_2.h>
#include <wincodec.h>
#include <wrl/client.h>
#include <vpl/mfxdispatcher.h>
#include <vpl/mfxjpeg.h>
#include <vpl/mfxvideo.h>
#include <cstdio>
#include <memory>
#include <stdexcept>
#include <vector>

using Microsoft::WRL::ComPtr;
static void require(bool ok, const char* message) {
    if (!ok) throw std::runtime_error(message);
}

// 使用系统 JPEG 编码器生成已知像素样本，避免另引 fixture 下载或自写 codec。
static std::vector<uint8_t> jpeg_sample() {
    ComPtr<IWICImagingFactory> factory;
    require(SUCCEEDED(CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER,
                                      IID_PPV_ARGS(&factory))), "WIC factory");
    ComPtr<IStream> stream;
    require(SUCCEEDED(CreateStreamOnHGlobal(nullptr, TRUE, &stream)), "sample stream");
    ComPtr<IWICBitmapEncoder> encoder;
    require(SUCCEEDED(factory->CreateEncoder(GUID_ContainerFormatJpeg, nullptr, &encoder)), "JPEG encoder");
    require(SUCCEEDED(encoder->Initialize(stream.Get(), WICBitmapEncoderNoCache)), "encoder init");
    ComPtr<IWICBitmapFrameEncode> frame;
    require(SUCCEEDED(encoder->CreateNewFrame(&frame, nullptr)), "encoder frame");
    require(SUCCEEDED(frame->Initialize(nullptr)), "frame init");
    require(SUCCEEDED(frame->SetSize(256, 128)), "frame size");
    WICPixelFormatGUID format = GUID_WICPixelFormat24bppBGR;
    require(SUCCEEDED(frame->SetPixelFormat(&format)) && IsEqualGUID(format, GUID_WICPixelFormat24bppBGR), "frame format");
    std::vector<uint8_t> pixels(256 * 128 * 3);
    for (size_t i = 0; i < pixels.size(); i += 3) {
        pixels[i] = 10; pixels[i + 1] = 40; pixels[i + 2] = 220;
    }
    require(SUCCEEDED(frame->WritePixels(128, 256 * 3, static_cast<UINT>(pixels.size()), pixels.data())), "frame pixels");
    require(SUCCEEDED(frame->Commit()) && SUCCEEDED(encoder->Commit()), "encoder commit");
    STATSTG stat{};
    require(SUCCEEDED(stream->Stat(&stat, STATFLAG_NONAME)) && stat.cbSize.QuadPart < 1024 * 1024, "sample length");
    std::vector<uint8_t> jpeg(static_cast<size_t>(stat.cbSize.QuadPart));
    require(SUCCEEDED(stream->Seek({}, STREAM_SEEK_SET, nullptr)), "sample rewind");
    ULONG read = 0;
    require(SUCCEEDED(stream->Read(jpeg.data(), static_cast<ULONG>(jpeg.size()), &read)) && read == jpeg.size(), "sample read");
    return jpeg;
}

static void describe_runtimes() {
    const auto loader = MFXLoad();
    require(loader != nullptr, "dispatcher loader");
    unsigned count = 0;
    for (mfxU32 index = 0; index < 64; ++index) {
        mfxHDL caps = nullptr;
        const auto status = MFXEnumImplementations(loader, index, MFX_IMPLCAPS_IMPLDESCSTRUCTURE, &caps);
        if (status != MFX_ERR_NONE) {
            std::printf("runtime_enumeration_end=%d count=%u\n", status, count);
            break;
        }
        const auto* desc = static_cast<const mfxImplDescription*>(caps);
        bool jpeg = false;
        for (mfxU16 i = 0; i < desc->Dec.NumCodecs; ++i)
            jpeg |= desc->Dec.Codecs[i].CodecID == MFX_CODEC_JPEG;
        std::printf("runtime=%u api=%u.%u implementation=%u acceleration=%u jpeg=%d\n", index,
            desc->ApiVersion.Major, desc->ApiVersion.Minor, desc->Impl, desc->AccelerationMode, jpeg);
        MFXDispReleaseImplDescription(loader, caps);
        caps = nullptr;
        const auto extended = MFXEnumImplementations(loader, index, MFX_IMPLCAPS_DEVICE_ID_EXTENDED, &caps);
        if (extended == MFX_ERR_NONE && caps) {
            const auto* id = static_cast<const mfxExtendedDeviceId*>(caps);
            std::printf("runtime_device=%04x:%04x luid_valid=%u node_mask=%u\n",
                id->VendorID, id->DeviceID, id->LUIDValid, id->LUIDDeviceNodeMask);
            MFXDispReleaseImplDescription(loader, caps);
        }
        ++count;
    }
    MFXUnload(loader);
}

int main() {
    std::setvbuf(stdout, nullptr, _IONBF, 0);
    const auto com = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    if (FAILED(com)) return 1;
    int exit = 0;
    try {
        describe_runtimes();
        ScrolleryVplResult result{};
        uint8_t invalid[] = {0xff, 0xd8, 0xff, 0xd9};
        std::vector<uint8_t> pixels(64 * 64 * 4);
        require(scrollery_vpl_decode(nullptr, invalid, sizeof(invalid), 64, pixels.data(),
            static_cast<uint32_t>(pixels.size()), &result) == 1, "missing runtime fallback");
        require(!scrollery_vpl_create(0xffffffff, -1, 0xffff, 0xffff, nullptr), "unmatched adapter rejected");
        auto jpeg = jpeg_sample();
        ComPtr<IDXGIFactory1> factory;
        require(SUCCEEDED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))), "DXGI factory");
        unsigned matched = 0, decoded = 0;
        for (UINT i = 0;; ++i) {
            ComPtr<IDXGIAdapter1> adapter;
            if (factory->EnumAdapters1(i, &adapter) == DXGI_ERROR_NOT_FOUND) break;
            DXGI_ADAPTER_DESC1 desc{};
            require(adapter && SUCCEEDED(adapter->GetDesc1(&desc)), "adapter description");
            if (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) continue;
            ScrolleryVplOpenResult opened{};
            void* session = scrollery_vpl_create(desc.AdapterLuid.LowPart, desc.AdapterLuid.HighPart,
                                                desc.VendorId, desc.DeviceId, &opened);
            std::printf("adapter=%04x:%04x matched_runtime=%d stage=%u status=%d\n", desc.VendorId, desc.DeviceId,
                session != nullptr, opened.stage, opened.status);
            if (!session) continue;
            std::unique_ptr<void, decltype(&scrollery_vpl_destroy)> owner(session, scrollery_vpl_destroy);
            ++matched;
            const auto status = scrollery_vpl_decode(session, jpeg.data(), static_cast<uint32_t>(jpeg.size()),
                64, pixels.data(), static_cast<uint32_t>(pixels.size()), &result);
            std::printf("status=%d header=%d query=%d init=%d decode=%d size=%ux%u\n", status,
                result.header_status, result.query_status, result.init_status, result.decode_status, result.width, result.height);
            if (status == 0) {
                ++decoded;
                require(result.width == 64 && result.height == 32, "target dimensions");
                const size_t center = (16 * 64 + 32) * 4;
                std::printf("center_rgba=%u,%u,%u,%u\n", pixels[center], pixels[center + 1], pixels[center + 2], pixels[center + 3]);
                require(pixels[center] >= 212 && pixels[center] <= 228 &&
                    pixels[center + 1] >= 32 && pixels[center + 1] <= 48 &&
                    pixels[center + 2] <= 18 && pixels[center + 3] == 255, "JPEG full-range RGB output");
                require(scrollery_vpl_decode(session, jpeg.data(), static_cast<uint32_t>(jpeg.size()),
                    64, pixels.data(), static_cast<uint32_t>(pixels.size()), &result) == 0,
                    "session reuse after releasing surfaces");
            }
            const auto bad = scrollery_vpl_decode(session, invalid, sizeof(invalid), 64, pixels.data(),
                static_cast<uint32_t>(pixels.size()), &result);
            std::printf("truncated_status=%d; closing_session\n", bad);
            owner.reset();
            require(bad != 0, "truncated JPEG rejected");
        }
        std::printf("fallback_checks=passed matched=%u hardware_samples=%u\n", matched, decoded);
    } catch (const std::exception& error) {
        std::fprintf(stderr, "%s\n", error.what());
        exit = 1;
    }
    CoUninitialize();
    return exit;
}
