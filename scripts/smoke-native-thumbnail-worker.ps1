# VideoSample 使用16:9 MP4夹具；默认仍生成带ICC/方向的JPEG。
param([switch]$RequireVpl, [string]$VideoSample)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$workerPath = Join-Path $repo 'target/debug/native-thumbnail-worker.exe'
$sampleDir = Join-Path $repo 'target/native-vpl/worker-smoke'
New-Item -ItemType Directory -Force -Path $sampleDir | Out-Null

if ($VideoSample) {
    if ($RequireVpl) { throw "RequireVpl applies to JPEG only" }
    $samplePath = (Resolve-Path -LiteralPath $VideoSample).Path
} else {
# 系统 JPEG 编码器生成 4:2:0 样本，再附标准 EXIF/ICC 元数据；不下载测试图。
Add-Type -AssemblyName System.Drawing
$bitmap = [System.Drawing.Bitmap]::new(256, 128)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
try {
    $graphics.Clear([System.Drawing.Color]::FromArgb(220, 40, 10))
    $samplePath = Join-Path $sampleDir 'oriented-icc.jpg'
    $bitmap.Save($samplePath, [System.Drawing.Imaging.ImageFormat]::Jpeg)
} finally {
    $graphics.Dispose()
    $bitmap.Dispose()
}
$jpeg = [System.IO.File]::ReadAllBytes($samplePath)
$icc = [System.IO.File]::ReadAllBytes((Join-Path $env:windir 'System32/spool/drivers/color/sRGB Color Space Profile.icm'))
$exif = [byte[]](0xff,0xe1,0,34,69,120,105,102,0,0,73,73,42,0,8,0,0,0,1,0,18,1,3,0,1,0,0,0,6,0,0,0,0,0,0,0)
$iccSize = $icc.Length + 16
if ($iccSize -gt 65535) { throw 'Test ICC exceeds one JPEG APP2 segment' }
$iccHeader = [byte[]](0xff,0xe2,($iccSize -shr 8),($iccSize -band 255),73,67,67,95,80,82,79,70,73,76,69,0,1,1)
[System.IO.File]::WriteAllBytes($samplePath, [byte[]]($jpeg[0..1] + $exif + $iccHeader + $icc + $jpeg[2..($jpeg.Length-1)]))

}

Add-Type -TypeDefinition @'
using System;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

public static class NativeThumbnailSmoke {
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool DuplicateHandle(IntPtr sourceProcess, IntPtr source, IntPtr targetProcess,
        out IntPtr target, uint access, bool inherit, uint options);
    [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
    static void Require(bool value, string message) { if (!value) throw new Exception(message); }
    static byte[] Read(BinaryReader reader, int size) {
        var bytes = reader.ReadBytes(size);
        Require(bytes.Length == size, "Truncated worker response");
        return bytes;
    }
    static void CheckDimensions(byte[] webp, bool video) {
        Require(webp.Length >= 30 && Encoding.ASCII.GetString(webp, 0, 4) == "RIFF", "WebP RIFF");
        int offset = 12;
        while (offset + 8 <= webp.Length) {
            int length = checked((int)BitConverter.ToUInt32(webp, offset + 4));
            int data = offset + 8;
            Require(length >= 0 && length <= webp.Length - data, "WebP chunk length");
            if (Encoding.ASCII.GetString(webp, offset, 4) == "VP8 ") {
                Require(length >= 10 && webp[data+3] == 0x9d && webp[data+4] == 1 && webp[data+5] == 0x2a, "VP8 frame header");
                int width = BitConverter.ToUInt16(webp, data+6) & 0x3fff;
                int height = BitConverter.ToUInt16(webp, data+8) & 0x3fff;
                Require(width == (video ? 64 : 32) && height == (video ? 36 : 64), "Expected 16:9 video or oriented JPEG: " + width + "x" + height);
                return;
            }
            offset = checked(data + length + (length & 1));
        }
        throw new Exception("Missing VP8 output");
    }
    public static void Run(string worker, string sample, bool requireVpl, bool video) {
        var start = new ProcessStartInfo(worker, video ? "--tail" : "--fast=1") {
            UseShellExecute=false, CreateNoWindow=true,
            RedirectStandardInput=true, RedirectStandardOutput=true, RedirectStandardError=true
        };
        using (var process = Process.Start(start)) {
            var errors = process.StandardError.ReadToEndAsync();
            // 同步原生调用可能不返回，测试本身也必须有进程级截止。
            using (var deadline = new Timer(_ => { try { process.Kill(); } catch {} }, null, 20000, Timeout.Infinite))
            using (var reader = new BinaryReader(process.StandardOutput.BaseStream))
            using (var writer = new BinaryWriter(process.StandardInput.BaseStream)) {
                try {
                    Require(Encoding.ASCII.GetString(Read(reader,4)) == "NTHH", "Worker protocol version");
                    for (int index = 0; index < 3; ++index) {
                        using (var input = File.OpenRead(sample)) {
                            IntPtr copied;
                            Require(DuplicateHandle(GetCurrentProcess(), input.SafeFileHandle.DangerousGetHandle(),
                                process.Handle, out copied, 0, false, 2), "DuplicateHandle failed");
                            string json = "{\"id\":" + (index+1) + ",\"file_handle\":" + copied.ToInt64() +
                                ",\"kind\":\"Image\",\"format\":\"jpg\",\"codec_hint\":null,\"prefer_gpu\":" +
                                (index == 1 ? "true" : "false") +
                                ",\"gpu_policy\":true,\"image_route\":\"Automatic\",\"decode_long_edge\":64,\"max_pixel_bytes\":11534336,\"output_size\":64,\"webp_quality\":90," +
                                "\"ai_cache_short_edge\":336,\"emit_ai_cache\":false,\"qos_foreground\":true,\"qos_revision\":1}";
                            if (video) json = json.Replace("\"kind\":\"Image\"", "\"kind\":\"VideoCover\"")
                                .Replace("\"format\":\"jpg\"", "\"format\":\"mp4\"");
                            if (index == 2) json = json.Replace("\"max_pixel_bytes\":11534336", "\"max_pixel_bytes\":1")
                                .Replace("\"qos_foreground\":true", "\"qos_foreground\":false")
                                .Replace("\"qos_revision\":1", "\"qos_revision\":2");
                            var request = Encoding.UTF8.GetBytes(json);
                            writer.Write((uint)request.Length); writer.Write(request); writer.Flush();
                            byte status;
                            byte completedStage = 0;
                            do {
                                status = reader.ReadByte();
                                Require(reader.ReadUInt64() == (ulong)(index+1), "Response identity");
                                if (status == 250) {
                                    byte stage = reader.ReadByte();
                                    Require(stage == completedStage + 1 && stage <= 3, "Resource stage order");
                                    completedStage = stage;
                                }
                            } while (status == 250);
                            Require(reader.ReadUInt64() == (index == 2 ? 2UL : 1UL), "QoS revision");
                            byte qosFlags = reader.ReadByte();
                            Require(qosFlags <= 3 && ((qosFlags & 2) != 0) == (index != 1), "QoS application only on revision change");
                            Require(reader.ReadByte() == 1, "QoS worker slot");
                            Require(reader.ReadUInt64() == (ulong)(index+1), "QoS sequence");
                            if (index == 2) {
                                Require(status == 4, "Pixel quota must reject actual decoded image");
                                Console.WriteLine("request=3 pixel_quota_rejected=true qos_revision=2 qos_attempted=true qos_accepted={0}", (qosFlags & 1) != 0);
                                continue;
                            }
                            Require(status == 0, "Worker failed with code " + status);
                            Require(completedStage == 3, "Source/GPU completed and bounded encoding ready before reply");
                            byte backend = reader.ReadByte();
                            uint vendor = reader.ReadUInt32(); uint device = reader.ReadUInt32();
                            Read(reader, 8); byte adapterKind = reader.ReadByte(); Read(reader, 64);
                            Require(adapterKind <= 2, "Adapter kind enum");
                            uint webpSize = reader.ReadUInt32(); ushort hashSize = reader.ReadUInt16(); uint aiSize = reader.ReadUInt32();
                            Require(webpSize <= 1048576 && hashSize <= 1024 && aiSize == 0, "Bounded payload");
                            CheckDimensions(Read(reader, checked((int)webpSize)), video); Read(reader, hashSize);
                            if (index == 0) Require(video ? backend == 5 : backend == 3 || backend == 4, "CPU preference invoked GPU");
                            if (video) Require((backend == 5 || backend == 6) && adapterKind == 0, "MF backend and unknown actual adapter");
                            if (index == 1 && requireVpl) Require(backend == 7 && vendor == 0x8086, "Expected actual VPL result");
                            Console.WriteLine("request={0} backend={1} device={2:x4}:{3:x4} adapter_kind={4} output={5}", index+1, backend, vendor, device, adapterKind, video ? "64x36 video" : "32x64 icc_input=true");
                        }
                    }
                    process.StandardInput.Close();
                    Require(process.WaitForExit(5000) && process.ExitCode == 0, "Worker clean shutdown");
                    Console.WriteLine("native_worker_smoke=passed");
                } finally {
                    if (!process.HasExited) process.Kill();
                }
            }
        }
    }
}
'@
[NativeThumbnailSmoke]::Run($workerPath, $samplePath, $RequireVpl.IsPresent, [bool]$VideoSample)
