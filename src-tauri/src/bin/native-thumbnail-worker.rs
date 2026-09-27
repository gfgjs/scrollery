//! 持久原生图片执行域。只消费宿主复制的文件句柄，不接收数据库或输出路径。

#[cfg(windows)]
fn main() -> std::io::Result<()> {
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};

    use scrollery_lib::thumbnail::native_protocol::{
        read_request, write_response, NativeQosAck, NativeResponse, WORKER_HELLO,
    };

    // stdout 专供二进制协议；原生 API 的错误仅写 stderr，供宿主诊断捕获。
    tracing_subscriber::fmt()
        .with_env_filter(if scrollery_lib::thumbnail::route_diagnostics::enabled() {
            "error,scrollery::thumb_routes=info"
        } else {
            "error"
        })
        .with_ansi(false)
        .with_writer(io::stderr)
        .init();

    let parallelism = std::env::args()
        .find_map(|arg| {
            arg.strip_prefix("--fast=")
                .and_then(|n| n.parse::<usize>().ok())
        })
        .unwrap_or(1)
        .clamp(1, 4);
    let mut input = io::stdin().lock();
    let output = Arc::new(Mutex::new(io::stdout()));
    {
        let mut output = output.lock().unwrap_or_else(|e| e.into_inner());
        output.write_all(&WORKER_HELLO)?;
        output.flush()?;
    }
    let (send, receive) = crossbeam_channel::bounded::<
        scrollery_lib::thumbnail::native_protocol::NativeRequest,
    >(parallelism);
    // scope 闭包拥有发送端：协议读取出错时也先关闭队列，再等待消费线程退出。
    std::thread::scope(move |scope| -> io::Result<()> {
        for worker_slot in 1..=parallelism {
            let receive = receive.clone();
            let output = Arc::clone(&output);
            scope.spawn(move || {
                scrollery_lib::engine::native::wic_engine::with_image_worker_session(|| {
                    let mut applied_qos: Option<(bool, u64, bool)> = None;
                    let mut sequence = 0u64;
                    while let Ok(request) = receive.recv() {
                        let id = request.id;
                        sequence += 1;
                        let qos = match applied_qos {
                            Some((foreground, revision, applied))
                                if foreground == request.qos_foreground
                                    && revision == request.qos_revision =>
                            {
                                NativeQosAck {
                                    worker_slot: worker_slot as u8,
                                    sequence,
                                    revision,
                                    applied,
                                    attempted: false,
                                }
                            }
                            _ => {
                                let applied =
                                    scrollery_lib::thumbnail::qos::apply_native_worker_qos(
                                        request.qos_foreground,
                                    );
                                applied_qos =
                                    Some((request.qos_foreground, request.qos_revision, applied));
                                NativeQosAck {
                                    worker_slot: worker_slot as u8,
                                    sequence,
                                    revision: request.qos_revision,
                                    applied,
                                    attempted: true,
                                }
                            }
                        };
                        let response = match process(request, |stage| {
                            let mut output = output.lock().unwrap_or_else(|e| e.into_inner());
                            if write_response(&mut *output, &NativeResponse::Stage { id, stage }).is_err() {
                                std::process::exit(71);
                            }
                        }) {
                            Ok((payload, execution, timings)) => NativeResponse::Ok {
                                id,
                                payload,
                                execution,
                                timings,
                                qos,
                            },
                            Err(code) => NativeResponse::Failed { id, code, qos },
                        };
                        if scrollery_lib::video::media_foundation::native_worker_has_abandoned_reader() {
                            // 超时 reader 仍可能持有 GPU 工作；不返回普通失败让宿主提前归还名额。
                            std::process::exit(70);
                        }
                        let mut output = output.lock().unwrap_or_else(|e| e.into_inner());
                        if write_response(&mut *output, &response).is_err() {
                            break;
                        }
                    }
                });
            });
        }
        while let Some(request) = read_request(&mut input)? {
            send.send(request)
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "worker stopped"))?;
        }
        drop(send);
        Ok(())
    })
}

#[cfg(windows)]
fn process(
    request: scrollery_lib::thumbnail::native_protocol::NativeRequest,
    mut stage: impl FnMut(scrollery_lib::thumbnail::native_protocol::NativeStage),
) -> Result<
    (
        scrollery_lib::thumbnail::generator::EncodedThumbPayload,
        scrollery_lib::thumbnail::native_protocol::NativeExecution,
        scrollery_lib::thumbnail::native_protocol::NativeTimings,
    ),
    u8,
> {
    use std::fs::File;
    use std::os::windows::io::{FromRawHandle, RawHandle};
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{GetFileType, FILE_TYPE_DISK};

    use scrollery_lib::engine::gpu::d2d_resize::{self, GpuImageOutcome};
    use scrollery_lib::engine::image_rs::ImageRsEngine;
    use scrollery_lib::engine::native::wic_engine::WicEngine;
    use scrollery_lib::engine::traits::ResizeHint;
    use scrollery_lib::thumbnail::generator::{encode_media_payload, ThumbConfig};
    use scrollery_lib::thumbnail::native_protocol::{
        NativeBackend, NativeExecution, NativeStage, NativeTimings,
    };

    let elapsed_us = |start: std::time::Instant| {
        u64::try_from(start.elapsed().as_micros())
            .unwrap_or(u64::MAX)
            .max(1)
    };

    if request.file_handle == 0
        || request.file_handle == u64::MAX
        || !matches!(request.output_size, 64 | 128 | 256 | 512 | 1024)
        || request.decode_long_edge == 0
        || request.decode_long_edge > 8192
        || request.max_pixel_bytes == 0
        || request.max_pixel_bytes > 128 * 1024 * 1024
        || !(1..=100).contains(&request.webp_quality)
    {
        return Err(2);
    }
    let handle = HANDLE(request.file_handle as usize as RawHandle);
    // SAFETY: 仅查询句柄类型；非法值会返回非磁盘类型，不能交给 File 接管。
    if unsafe { GetFileType(handle) } != FILE_TYPE_DISK {
        return Err(2);
    }
    // SAFETY:此数值只由宿主 DuplicateHandle 到当前进程后传入；本分支接管并关闭句柄。
    let mut file = unsafe { File::from_raw_handle(request.file_handle as usize as RawHandle) };
    let format = request.format.to_ascii_lowercase();
    let video = matches!(
        request.kind,
        scrollery_lib::thumbnail::native_protocol::NativeKind::VideoCover
    );
    if !video && !request.prefer_gpu && !request.gpu_policy {
        // 显式 CPU 图片策略不再使用旧 GPU 会话；GPU 名额临时不足仍保留原生缓存。
        scrollery_lib::engine::native::wic_engine::release_image_gpu_sessions();
    }
    if !video && matches!(format.as_str(), "jpg" | "jpeg") {
        let embedded_started = std::time::Instant::now();
        let mut embedded_released = false;
        if let Some((webp, thumbhash)) = scrollery_lib::thumbnail::exif_thumb::try_exif_thumb_file(
            &mut file,
            request.output_size,
            request.webp_quality,
            request.max_pixel_bytes,
            || {
                stage(NativeStage::GpuDone);
                stage(NativeStage::SourceDone);
                embedded_released = true;
            },
        ) {
            return Ok((
                scrollery_lib::thumbnail::generator::EncodedThumbPayload {
                    webp,
                    thumbhash,
                    ai_cache: None,
                },
                NativeExecution::cpu(NativeBackend::EmbeddedJpeg),
                NativeTimings {
                    embedded_jpeg_combined_us: elapsed_us(embedded_started),
                    ..NativeTimings::default()
                },
            ));
        }
        // 进入编码前已释放源许可时，编码失败不能重新读主图。
        if embedded_released {
            return Err(5);
        }
    }
    let decode_started = std::time::Instant::now();
    let hint = Some(ResizeHint::LongEdge(request.decode_long_edge));
    // 宿主已预留同时存活的缓冲空间；系统 codec 隐藏缓存另受 Job 配额约束。
    let max_cpu_decode_bytes = request.max_pixel_bytes;
    let max_wic_output_bytes = request.max_pixel_bytes.min(64 * 1024 * 1024);
    let mut execution = NativeExecution::cpu(NativeBackend::ImageRs);
    let vpl_decoded = {
        #[cfg(feature = "native-vpl")]
        {
            if !video && request.prefer_gpu && matches!(format.as_str(), "jpg" | "jpeg") {
                file.try_clone().ok().and_then(|source| {
                    match scrollery_lib::engine::gpu::vpl_jpeg::decode_open_file(source, request.decode_long_edge, request.max_pixel_bytes) {
                        Ok(Some((decoded, adapter))) => {
                            execution = NativeExecution { backend: NativeBackend::ImageVpl,
                                vendor_id: adapter.vendor_id, device_id: adapter.device_id,
                                luid_high: adapter.luid_high, luid_low: adapter.luid_low,
                        adapter_kind: Default::default() };
                            Some(decoded)
                        }
                        Ok(None) => None,
                        Err(error) => {
                            tracing::debug!(target: "scrollery::thumb_perf", %error, "VPL JPEG route unavailable");
                            None
                        }
                    }
                })
            } else {
                None
            }
        }
        #[cfg(not(feature = "native-vpl"))]
        {
            if !video && request.prefer_gpu {
                use scrollery_lib::thumbnail::route_diagnostics::{record, Reason, Stage};
                record(Stage::Vpl, Reason::NotCompiled, std::time::Duration::ZERO);
            }
            None
        }
    };
    #[cfg(feature = "native-vpl")]
    if scrollery_lib::engine::gpu::vpl_jpeg::has_abandoned_work() {
        std::process::exit(70);
    }
    let gpu_decoded = if vpl_decoded.is_some() {
        vpl_decoded
    } else if !video && request.prefer_gpu {
        file.try_clone().ok().and_then(|source| {
            match d2d_resize::decode_open_file(source, &format, request.decode_long_edge, request.max_pixel_bytes) {
                Ok(GpuImageOutcome::Scaled(decoded, adapter)) => {
                    execution = NativeExecution {
                        backend: NativeBackend::ImageD2d,
                        vendor_id: adapter.vendor_id,
                        device_id: adapter.device_id,
                        luid_high: adapter.luid_high,
                        luid_low: adapter.luid_low,
                        adapter_kind: Default::default(),
                    };
                    Some(decoded)
                }
                // WIC 现保留 ICC；这里只跳过 GPU 变换，沿用下方原生 CPU 优先级。
                Ok(GpuImageOutcome::NeedsIccCpu | GpuImageOutcome::Unavailable) => None,
                Err(error) => {
                    tracing::debug!(target: "scrollery::thumb_perf", %error, "image GPU route unavailable");
                    None
                }
            }
        })
    } else {
        None
    };
    if !video {
        stage(NativeStage::GpuDone);
    }
    let decoded = if video {
        execution = NativeExecution::cpu(NativeBackend::VideoMf);
        if !scrollery_lib::video::native_cover_formats().contains(&format.as_str()) {
            return Err(3);
        }
        scrollery_lib::video::media_foundation::cover_open_file(
            file,
            &format,
            request.decode_long_edge,
            request.codec_hint.as_deref(),
            request.prefer_gpu,
            request.max_pixel_bytes,
        )
        .map(|(decoded, fact)| {
            let adapter = fact.candidate_adapter;
            execution = NativeExecution {
                backend: if fact.hardware_decoder_mft {
                    NativeBackend::VideoMfHardwareMft
                } else {
                    NativeBackend::VideoMf
                },
                vendor_id: adapter.map_or(0, |value| value.vendor_id),
                device_id: adapter.map_or(0, |value| value.device_id),
                luid_high: adapter.map_or(0, |value| value.luid_high),
                luid_low: adapter.map_or(0, |value| value.luid_low),
                adapter_kind: Default::default(),
            };
            decoded
        })
    } else if let Some(decoded) = gpu_decoded {
        Ok(decoded)
    } else {
        match format.as_str() {
            "jpg" | "jpeg" | "png" | "webp" | "bmp" | "gif" | "tif" | "tiff" => {
                ImageRsEngine::decode_open_file_bounded(file, &format, hint, max_cpu_decode_bytes)
            }
            "heic" | "heif" | "avif" | "ico" => {
                execution = NativeExecution::cpu(NativeBackend::ImageWic);
                WicEngine::decode_open_file_bounded(file, &format, hint, max_wic_output_bytes)
            }
            _ => return Err(3),
        }
    }
    .map_err(|error| {
        tracing::error!(request_id = request.id, video, prefer_gpu = request.prefer_gpu,
            max_pixel_bytes = request.max_pixel_bytes, %error, "native thumbnail decode failed");
        4u8
    })?;
    if scrollery_lib::video::media_foundation::native_worker_has_abandoned_reader() {
        std::process::exit(70);
    }
    if video {
        stage(NativeStage::GpuDone);
    }
    stage(NativeStage::SourceDone);
    let decode_transform_us = elapsed_us(decode_started);
    let config = ThumbConfig {
        cache_dir: std::path::PathBuf::new(),
        size: request.output_size,
        skip_max_bytes: 0,
        strategy: "cpu".into(),
        ai_hq_cache: request.emit_ai_cache,
        webp_quality: request.webp_quality,
        ai_cache_short_edge: request.ai_cache_short_edge,
        output_fingerprint: None,
    };
    execution.resolve_adapter_kind();
    let encode_started = std::time::Instant::now();
    let mut encode_attempt = scrollery_lib::thumbnail::route_diagnostics::Attempt::new(
        scrollery_lib::thumbnail::route_diagnostics::Stage::Encode,
    );
    encode_media_payload(0, decoded, &config, request.emit_ai_cache)
        .map(|payload| {
            encode_attempt.reason = scrollery_lib::thumbnail::route_diagnostics::Reason::Success;
            (
                payload,
                execution,
                NativeTimings {
                    decode_transform_us,
                    encode_hash_us: elapsed_us(encode_started),
                    embedded_jpeg_combined_us: 0,
                },
            )
        })
        .map_err(|_| 5)
}

#[cfg(not(windows))]
fn main() {}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use scrollery_lib::thumbnail::native_protocol::{
        NativeBackend, NativeKind, NativeRequest, NativeStage,
    };
    use std::io::Write;
    use std::os::windows::io::IntoRawHandle;

    #[test]
    fn embedded_route_is_independent_of_gpu_admission() {
        let mut embedded = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut embedded)
            .encode_image(&image::RgbImage::new(160, 120))
            .unwrap();
        // IFD0 带方向 6；IFD1 指向同一 APP1 内的缩略图，覆盖探测后复用句柄。
        let mut exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0".to_vec();
        exif.extend_from_slice(&[18, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0]);
        exif.extend_from_slice(&26u32.to_le_bytes());
        exif.extend_from_slice(&2u16.to_le_bytes());
        exif.extend_from_slice(&[1, 2, 4, 0, 1, 0, 0, 0]);
        exif.extend_from_slice(&56u32.to_le_bytes());
        exif.extend_from_slice(&[2, 2, 4, 0, 1, 0, 0, 0]);
        exif.extend_from_slice(&(embedded.len() as u32).to_le_bytes());
        exif.extend_from_slice(&[0; 4]);
        exif.extend_from_slice(&embedded);
        let mut main = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut main)
            .encode_image(&image::RgbImage::new(600, 400))
            .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("embedded.jpg");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(&main[..2]).unwrap();
        file.write_all(&[255, 225]).unwrap();
        file.write_all(&((exif.len() + 2) as u16).to_be_bytes())
            .unwrap();
        file.write_all(&exif).unwrap();
        file.write_all(&main[2..]).unwrap();
        drop(file);
        let run = |gpu_policy, prefer_gpu, size| {
            let mut stages = Vec::new();
            let request = NativeRequest {
                id: 1,
                file_handle: std::fs::File::open(&path).unwrap().into_raw_handle() as usize as u64,
                kind: NativeKind::Image,
                format: "jpg".into(),
                codec_hint: None,
                gpu_policy,
                prefer_gpu,
                decode_long_edge: size,
                output_size: size,
                max_pixel_bytes: 64 * 1024 * 1024,
                webp_quality: 80,
                ai_cache_short_edge: 336,
                emit_ai_cache: false,
                qos_foreground: true,
                qos_revision: 1,
            };
            let result = process(request, |stage| stages.push(stage)).unwrap();
            assert_eq!(stages, [NativeStage::GpuDone, NativeStage::SourceDone]);
            result
        };
        let cpu = run(false, false, 256);
        let gpu = run(true, true, 256);
        let denied = run(true, false, 256);
        assert_eq!(cpu.1.backend, NativeBackend::EmbeddedJpeg);
        assert_eq!(gpu.1.backend, NativeBackend::EmbeddedJpeg);
        assert_eq!(denied.1.backend, NativeBackend::EmbeddedJpeg);
        assert_eq!(cpu.0.webp, gpu.0.webp);
        assert_eq!(cpu.0.webp, denied.0.webp);
        let embedded_image = image::load_from_memory(&cpu.0.webp).unwrap();
        assert_eq!(
            (embedded_image.width(), embedded_image.height()),
            (192, 256)
        );
        let full = run(true, false, 512);
        assert_eq!(full.1.backend, NativeBackend::ImageRs);
        let full_image = image::load_from_memory(&full.0.webp).unwrap();
        assert_eq!((full_image.width(), full_image.height()), (341, 512));
    }
}
