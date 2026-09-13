// crates/exotic-protocol/src/message/tests.rs
//! 消息类型序列化往返测试(自 message.rs 结构性拆分,tierB-1)。

use super::*;

#[test]
fn request_tagged_roundtrip() {
    let req = RequestBody::Thumbnail {
        item_id: 42,
        source_path: "a.psd".into(),
        target_long_edge: 480,
        input_fingerprint: "fp".into(),
    };
    let s = serde_json::to_string(&req).unwrap();
    assert!(s.contains(r#""op":"thumbnail""#));
    let back: RequestBody = serde_json::from_str(&s).unwrap();
    assert_eq!(back, req);
    assert_eq!(back.item_id(), Some(42));
    assert_eq!(back.input_fingerprint(), Some("fp"));
}

#[test]
fn error_code_serde_and_retryable() {
    assert_eq!(
        serde_json::to_string(&WorkerErrorCode::UnsupportedVariant).unwrap(),
        r#""unsupported_variant""#
    );
    assert!(!WorkerErrorCode::UnsupportedVariant.default_retryable());
    assert!(WorkerErrorCode::IoError.default_retryable());
    assert!(WorkerErrorCode::InternalError.default_retryable());
    assert!(!WorkerErrorCode::MalformedInput.default_retryable());
}

#[test]
fn session_init_roundtrip_and_tags() {
    let req = RequestBody::SessionInit {
        session_id: 7,
        models: vec![
            ModelDescriptor {
                role: ModelRole::ImageEncoder,
                handle: ModelHandle::Path("C:/models/img.onnx".into()),
                len: 10,
                sha256: "ab".repeat(32),
                model_id: None,
            },
            ModelDescriptor {
                role: ModelRole::FaceRecog,
                handle: ModelHandle::Named("pn-0011223344556677-ff".into()),
                len: 20,
                sha256: "cd".repeat(32),
                model_id: None,
            },
        ],
        model_profile: ModelProfileSnapshot {
            arch_id: "clip-vit".into(),
            image_file: "img.onnx".into(),
            text_file: "txt.onnx".into(),
            batch_size: 32,
            face_profile_id: Some("yunet-sface".into()),
        },
        models_root: "C:/models".into(),
        ai_cache_dir: "C:/cache/ai".into(),
        image_provider: "directml".into(),
    };
    let s = serde_json::to_string(&req).unwrap();
    assert!(s.contains(r#""op":"session_init""#));
    assert!(s.contains(r#""kind":"path""#));
    assert!(s.contains(r#""kind":"named""#));
    assert!(s.contains(r#""role":"image_encoder""#));
    let back: RequestBody = serde_json::from_str(&s).unwrap();
    assert_eq!(back, req);
    // 会话 op 无单项语义。
    assert_eq!(back.item_id(), None);
    assert_eq!(back.input_fingerprint(), None);
}

#[test]
fn batch_ops_roundtrip_and_tags() {
    let embed = RequestBody::EmbedBatch {
        items: vec![EmbedItem {
            item_id: 1,
            cache_key: "aabbcc".into(),
            fingerprint: "fp1".into(),
        }],
    };
    let s = serde_json::to_string(&embed).unwrap();
    assert!(s.contains(r#""op":"embed_batch""#));
    assert_eq!(serde_json::from_str::<RequestBody>(&s).unwrap(), embed);
    assert_eq!(embed.item_id(), None);

    let face = RequestBody::FaceDetectEmbed {
        items: vec![FaceItem {
            item_id: 2,
            cache_key: None,
            source_path: Some("D:/photos/a.jpg".into()),
            fingerprint: "fp2".into(),
        }],
        det_score_thresh: 0.9,
    };
    let s = serde_json::to_string(&face).unwrap();
    assert!(s.contains(r#""op":"face_detect_embed""#));
    assert_eq!(serde_json::from_str::<RequestBody>(&s).unwrap(), face);

    let close = RequestBody::SessionClose { session_id: 9 };
    let s = serde_json::to_string(&close).unwrap();
    assert!(s.contains(r#""op":"session_close""#));
    assert_eq!(serde_json::from_str::<RequestBody>(&s).unwrap(), close);

    // EncodeText(T17 additive):无单项语义;应答体 count 往返一致。
    let enc = RequestBody::EncodeText {
        texts: vec!["海边日落".into(), "cat".into()],
    };
    let s = serde_json::to_string(&enc).unwrap();
    assert!(s.contains(r#""op":"encode_text""#));
    assert_eq!(serde_json::from_str::<RequestBody>(&s).unwrap(), enc);
    assert_eq!(enc.item_id(), None);
    assert_eq!(enc.input_fingerprint(), None);
    let te = TextEmbedSuccess { count: 2 };
    let s = serde_json::to_string(&te).unwrap();
    assert_eq!(serde_json::from_str::<TextEmbedSuccess>(&s).unwrap(), te);
}

#[test]
fn face_item_result_dims_roundtrip_and_legacy_default() {
    // face 波 additive:Ok 补 width/height,往返一致。
    let ok = FaceItemResult::Ok {
        item_id: 7,
        fingerprint: "fp7".into(),
        faces: vec![],
        width: 800,
        height: 600,
    };
    let s = serde_json::to_string(&ok).unwrap();
    assert_eq!(serde_json::from_str::<FaceItemResult>(&s).unwrap(), ok);
    // 旧帧(无 width/height)仍可解析,serde default 落 0——host 校验层负责拒收 0。
    let legacy = r#"{"status":"ok","item_id":7,"fingerprint":"fp7","faces":[]}"#;
    match serde_json::from_str::<FaceItemResult>(legacy).unwrap() {
        FaceItemResult::Ok { width, height, .. } => assert_eq!((width, height), (0, 0)),
        _ => panic!("期望 Ok"),
    }
}

#[test]
fn success_body_thumbnail_wire_shape_unchanged() {
    // v1 时代 thumbnail Success 的线上形状在 v2 下不变:新增三字段 None 时不序列化。
    let body = SuccessBody {
        item_id: Some(1),
        input_fingerprint: Some("fp".into()),
        mime: Some("image/webp".into()),
        width: Some(10),
        height: Some(20),
        metadata: None,
        session: None,
        embed: None,
        face: None,
        text_embed: None,
        ocr_session: None,
        ocr: None,
        enhance: None,
        video_session: None,
        video_probe: None,
        video_out: None,
        video_frames: None,
    };
    let s = serde_json::to_string(&body).unwrap();
    assert!(!s.contains("session"));
    assert!(!s.contains("embed"));
    assert!(!s.contains("face"));
    assert!(!s.contains("text_embed"));
    assert!(!s.contains("ocr_session"));
    assert!(!s.contains(r#""ocr""#));
    assert!(!s.contains("enhance"));
    assert!(!s.contains("video_"));
    assert!(s.contains(r#""item_id":1"#));
    // v1 形状 JSON(无新字段)仍可解析。
    let legacy = r#"{"item_id":2,"input_fingerprint":"f","mime":null,"width":null,"height":null,"metadata":null}"#;
    let back: SuccessBody = serde_json::from_str(legacy).unwrap();
    assert_eq!(back.item_id, Some(2));
    assert!(back.session.is_none() && back.embed.is_none() && back.face.is_none());
    assert!(back.text_embed.is_none());
    assert!(back.ocr_session.is_none() && back.ocr.is_none());
    assert!(back.enhance.is_none());
    assert!(back.video_session.is_none() && back.video_probe.is_none() && back.video_out.is_none());
    assert!(back.video_frames.is_none());
}

#[test]
fn success_body_video_wire_shape_unchanged() {
    // 视频格式扩展子系统 design.md §2.3:新增 video_* 四字段 None 时不序列化,同 enhance 波先例。
    let body = SuccessBody {
        item_id: None,
        input_fingerprint: None,
        mime: None,
        width: None,
        height: None,
        metadata: None,
        session: None,
        embed: None,
        face: None,
        text_embed: None,
        ocr_session: None,
        ocr: None,
        enhance: None,
        video_session: None,
        video_probe: None,
        video_out: None,
        video_frames: None,
    };
    let s = serde_json::to_string(&body).unwrap();
    assert!(!s.contains("video_session"));
    assert!(!s.contains("video_probe"));
    assert!(!s.contains("video_out"));
    assert!(!s.contains("video_frames"));

    let with_session = SuccessBody {
        video_session: Some(VideoSessionInfo {
            caps: vec!["video_probe".into(), "video_remux".into()],
            ffmpeg_version: "n7.1".into(),
        }),
        ..SuccessBody::default()
    };
    let s = serde_json::to_string(&with_session).unwrap();
    assert!(s.contains(r#""video_session""#));
    assert!(s.contains(r#""ffmpeg_version":"n7.1""#));
    let back: SuccessBody = serde_json::from_str(&s).unwrap();
    assert_eq!(back.video_session, with_session.video_session);

    let with_probe = SuccessBody {
        video_probe: Some(VideoProbeInfo {
            container: "matroska,webm".into(),
            duration_ms: Some(60_000),
            width: Some(1920),
            height: Some(1080),
            rotation: Some(0),
            fps: Some(29.97),
            bitrate: Some(4_000_000),
            video_codec: "h264".into(),
            video_profile: Some("high".into()),
            bit_depth: Some(8),
            pixel_format: Some("yuv420p".into()),
            audio_tracks: vec![VideoAudioTrack {
                index: 0,
                codec: "aac".into(),
                channels: Some(2),
                language: Some("und".into()),
                is_default: true,
            }],
            has_subtitles: false,
            has_hdr_metadata: false,
        }),
        ..SuccessBody::default()
    };
    let s = serde_json::to_string(&with_probe).unwrap();
    assert!(s.contains(r#""video_probe""#));
    assert!(s.contains(r#""container":"matroska,webm""#));
    assert!(s.contains(r#""audio_tracks""#));
    let back: SuccessBody = serde_json::from_str(&s).unwrap();
    assert_eq!(back.video_probe, with_probe.video_probe);

    let with_out = SuccessBody {
        video_out: Some(VideoOutInfo {
            out_bytes: 12345,
            out_duration_ms: 60_000,
            video_copied: true,
            audio_copied: false,
        }),
        ..SuccessBody::default()
    };
    let s = serde_json::to_string(&with_out).unwrap();
    assert!(s.contains(r#""video_out""#));
    assert!(s.contains(r#""out_bytes":12345"#));
    let back: SuccessBody = serde_json::from_str(&s).unwrap();
    assert_eq!(back.video_out, with_out.video_out);

    let with_frames = SuccessBody {
        video_frames: Some(VideoFramesInfo {
            cell_width: 160,
            cell_height: 90,
            n: 9,
        }),
        ..SuccessBody::default()
    };
    let s = serde_json::to_string(&with_frames).unwrap();
    assert!(s.contains(r#""video_frames""#));
    assert!(s.contains(r#""cell_width":160"#));
    let back: SuccessBody = serde_json::from_str(&s).unwrap();
    assert_eq!(back.video_frames, with_frames.video_frames);
}

#[test]
fn success_body_ocr_session_and_batch_wire_shape() {
    // OCR 会话/批量成功应答的键名锁定(防 rename)。
    let body = SuccessBody {
        item_id: None,
        input_fingerprint: None,
        mime: None,
        width: None,
        height: None,
        metadata: None,
        session: None,
        embed: None,
        face: None,
        text_embed: None,
        ocr_session: Some(OcrSessionReadyBody {
            caps: vec!["ocr_text".into()],
        }),
        ocr: Some(OcrBatchSuccess { results: vec![] }),
        enhance: None,
        video_session: None,
        video_probe: None,
        video_out: None,
        video_frames: None,
    };
    let s = serde_json::to_string(&body).unwrap();
    assert!(s.contains(r#""ocr_session""#));
    assert!(s.contains(r#""ocr""#));
}

#[test]
fn success_body_enhance_wire_shape() {
    // EnhanceRun 完成回执键名锁定(防 rename)。
    let body = SuccessBody {
        item_id: None,
        input_fingerprint: None,
        mime: None,
        width: None,
        height: None,
        metadata: None,
        session: None,
        embed: None,
        face: None,
        text_embed: None,
        ocr_session: None,
        ocr: None,
        enhance: Some(EnhanceDone {
            out_width: 2048,
            out_height: 2048,
            tiles_total: 16,
        }),
        video_session: None,
        video_probe: None,
        video_out: None,
        video_frames: None,
    };
    let s = serde_json::to_string(&body).unwrap();
    assert!(s.contains(r#""enhance""#));
    assert!(s.contains(r#""out_width":2048"#));
    assert!(s.contains(r#""tiles_total":16"#));
    let back: SuccessBody = serde_json::from_str(&s).unwrap();
    assert_eq!(
        back.enhance,
        Some(EnhanceDone {
            out_width: 2048,
            out_height: 2048,
            tiles_total: 16,
        })
    );
}

#[test]
fn ocr_ops_roundtrip_and_tags() {
    let init = RequestBody::OcrSessionInit {
        session_id: 11,
        models: vec![ModelDescriptor {
            role: ModelRole::OcrDet,
            handle: ModelHandle::Path("C:/models/ocr_det.onnx".into()),
            len: 10,
            sha256: "ab".repeat(32),
            model_id: None,
        }],
        ocr_profile_id: "pp-ocrv5-mobile".into(),
        models_root: "C:/models".into(),
    };
    let s = serde_json::to_string(&init).unwrap();
    assert!(s.contains(r#""op":"ocr_session_init""#));
    assert!(s.contains(r#""role":"ocr_det""#));
    assert_eq!(serde_json::from_str::<RequestBody>(&s).unwrap(), init);
    assert_eq!(init.item_id(), None);
    assert_eq!(init.input_fingerprint(), None);

    let close = RequestBody::OcrSessionClose { session_id: 11 };
    let s = serde_json::to_string(&close).unwrap();
    assert!(s.contains(r#""op":"ocr_session_close""#));
    assert_eq!(serde_json::from_str::<RequestBody>(&s).unwrap(), close);

    let batch = RequestBody::OcrBatch {
        items: vec![OcrItem {
            item_id: 3,
            cache_key: None,
            source_path: Some("D:/photos/a.jpg".into()),
            fingerprint: "fp3".into(),
        }],
    };
    let s = serde_json::to_string(&batch).unwrap();
    assert!(s.contains(r#""op":"ocr_batch""#));
    assert_eq!(serde_json::from_str::<RequestBody>(&s).unwrap(), batch);
    assert_eq!(batch.item_id(), None);
    assert_eq!(batch.input_fingerprint(), None);
}

#[test]
fn ocr_item_result_status_tags_and_ocr_line_shape() {
    let ok = OcrItemResult::Ok {
        item_id: 1,
        fingerprint: "f1".into(),
        lines: vec![OcrLine {
            text: "hello".into(),
            quad: [[0.0, 0.0], [10.0, 0.0], [10.0, 5.0], [0.0, 5.0]],
            confidence: 0.9,
        }],
        width: 100,
        height: 50,
    };
    let err = OcrItemResult::Err {
        item_id: 2,
        fingerprint: "f2".into(),
        code: WorkerErrorCode::MalformedInput,
    };
    let s = serde_json::to_string(&vec![ok.clone(), err.clone()]).unwrap();
    assert!(s.contains(r#""status":"ok""#));
    assert!(s.contains(r#""status":"err""#));
    assert!(s.contains(r#""code":"malformed_input""#));
    assert!(s.contains(r#""text":"hello""#));
    assert!(s.contains(r#""quad""#));
    assert!(s.contains(r#""confidence":0.9"#));
    let back: Vec<OcrItemResult> = serde_json::from_str(&s).unwrap();
    assert_eq!(back, vec![ok, err]);
}

#[test]
fn ocr_model_role_serde_tags() {
    assert_eq!(
        serde_json::to_string(&ModelRole::OcrDet).unwrap(),
        r#""ocr_det""#
    );
    assert_eq!(
        serde_json::to_string(&ModelRole::OcrCls).unwrap(),
        r#""ocr_cls""#
    );
    assert_eq!(
        serde_json::to_string(&ModelRole::OcrRec).unwrap(),
        r#""ocr_rec""#
    );
    assert_eq!(
        serde_json::to_string(&ModelRole::OcrDict).unwrap(),
        r#""ocr_dict""#
    );
}

#[test]
fn enhance_model_role_serde_tag() {
    assert_eq!(
        serde_json::to_string(&ModelRole::Enhance).unwrap(),
        r#""enhance""#
    );
}

#[test]
fn enhance_ops_roundtrip_and_tags() {
    let init = RequestBody::EnhanceSessionInit {
        session_id: 21,
        models: vec![ModelDescriptor {
            role: ModelRole::Enhance,
            handle: ModelHandle::Path("C:/models/realesrgan-x4plus-fp16.onnx".into()),
            len: 12,
            sha256: "ef".repeat(32),
            model_id: Some("realesrgan-x4plus".into()),
        }],
        models_root: "C:/models".into(),
        work_dir: "D:/cache/enhance".into(),
    };
    let s = serde_json::to_string(&init).unwrap();
    assert!(s.contains(r#""op":"enhance_session_init""#));
    assert!(s.contains(r#""role":"enhance""#));
    // Enhance 路径 model_id 必须在场序列化(worker 靠它寻址具体模型)。
    assert!(s.contains(r#""model_id":"realesrgan-x4plus""#));
    // models_root 归属校验根随会话下发(worker 据此拒 ModelHandle::Path 越界)。
    assert!(s.contains(r#""models_root":"C:/models""#));
    // work_dir 输出白名单前缀随会话下发(worker 据此拒 output_tmp_path 越界)。
    assert!(s.contains(r#""work_dir":"D:/cache/enhance""#));
    assert_eq!(serde_json::from_str::<RequestBody>(&s).unwrap(), init);
    assert_eq!(init.item_id(), None);
    assert_eq!(init.input_fingerprint(), None);

    let close = RequestBody::EnhanceSessionClose { session_id: 21 };
    let s = serde_json::to_string(&close).unwrap();
    assert!(s.contains(r#""op":"enhance_session_close""#));
    assert_eq!(serde_json::from_str::<RequestBody>(&s).unwrap(), close);

    let run = RequestBody::EnhanceRun {
        session_id: 21,
        source_path: "D:/photos/a.jpg".into(),
        output_tmp_path: "D:/cache/enhance/job1.tmp".into(),
        output_format: "jpeg".into(),
        steps: vec![
            EnhanceStep {
                task: EnhanceTask::Denoise,
                model_id: "drunet".into(),
                strength: Some(15.0),
            },
            EnhanceStep {
                task: EnhanceTask::DejpegArtifact,
                model_id: "fbcnn".into(),
                strength: Some(60.0),
            },
            EnhanceStep {
                task: EnhanceTask::Upscale,
                model_id: "realesrgan-x4plus".into(),
                strength: None,
            },
        ],
    };
    let s = serde_json::to_string(&run).unwrap();
    assert!(s.contains(r#""op":"enhance_run""#));
    assert!(s.contains(r#""task":"denoise""#));
    assert!(s.contains(r#""task":"dejpeg_artifact""#));
    assert!(s.contains(r#""task":"upscale""#));
    assert_eq!(serde_json::from_str::<RequestBody>(&s).unwrap(), run);
    assert_eq!(run.item_id(), None);
    assert_eq!(run.input_fingerprint(), None);

    // strength=None 的单步:锁 skip_serializing_if 行为,防退化成 "strength":null。
    let no_strength = EnhanceStep {
        task: EnhanceTask::Upscale,
        model_id: "realesrgan-x4plus".into(),
        strength: None,
    };
    let s_none = serde_json::to_string(&no_strength).unwrap();
    assert!(!s_none.contains(r#""strength""#));
}

#[test]
fn video_session_ops_roundtrip_and_tags() {
    let init = RequestBody::VideoSessionInit {
        session_id: 31,
        ffmpeg_exe_path: "C:/tools/ffmpeg/ffmpeg.exe".into(),
        ffmpeg_sha256: "ab".repeat(32),
        work_dir: "D:/cache/video".into(),
    };
    let s = serde_json::to_string(&init).unwrap();
    assert!(s.contains(r#""op":"video_session_init""#));
    assert!(s.contains(r#""work_dir":"D:/cache/video""#));
    assert_eq!(serde_json::from_str::<RequestBody>(&s).unwrap(), init);
    assert_eq!(init.item_id(), None);
    assert_eq!(init.input_fingerprint(), None);

    let close = RequestBody::VideoSessionClose { session_id: 31 };
    let s = serde_json::to_string(&close).unwrap();
    assert!(s.contains(r#""op":"video_session_close""#));
    assert_eq!(serde_json::from_str::<RequestBody>(&s).unwrap(), close);
}

#[test]
fn video_probe_roundtrip_and_tags() {
    let probe = RequestBody::VideoProbe {
        session_id: 31,
        source_path: "D:/videos/a.rmvb".into(),
        input_fingerprint: "fp-video".into(),
    };
    let s = serde_json::to_string(&probe).unwrap();
    assert!(s.contains(r#""op":"video_probe""#));
    assert_eq!(serde_json::from_str::<RequestBody>(&s).unwrap(), probe);
    // VideoProbe 无 item_id 概念(非按 item 派活),`input_fingerprint()` 方法只服务
    // Thumbnail/Metadata 的单值语义,VideoProbe 走 None 分支(理由同 SessionInit 等
    // 会话化 op)。
    assert_eq!(probe.item_id(), None);
    assert_eq!(probe.input_fingerprint(), None);
}

#[test]
fn video_remux_and_transcode_roundtrip_and_tags() {
    let remux = RequestBody::VideoRemux {
        session_id: 31,
        source_path: "D:/videos/a.mkv".into(),
        output_tmp_path: "D:/cache/video/job1.mp4.tmp".into(),
        audio_transcode: true,
        audio_track_index: Some(0),
    };
    let s = serde_json::to_string(&remux).unwrap();
    assert!(s.contains(r#""op":"video_remux""#));
    assert!(s.contains(r#""audio_transcode":true"#));
    assert_eq!(serde_json::from_str::<RequestBody>(&s).unwrap(), remux);
    assert_eq!(remux.item_id(), None);
    assert_eq!(remux.input_fingerprint(), None);

    let transcode = RequestBody::VideoTranscode {
        session_id: 31,
        source_path: "D:/videos/a.mkv".into(),
        output_tmp_path: "D:/cache/video/job2.mp4.tmp".into(),
        encoder_ladder: vec!["h264_nvenc".into(), "h264_mf".into()],
        crf: Some(23),
        bitrate_kbps: None,
        max_long_edge: Some(1920),
        audio_track_index: None,
        hw_decode: true,
    };
    let s = serde_json::to_string(&transcode).unwrap();
    assert!(s.contains(r#""op":"video_transcode""#));
    assert!(s.contains(r#""encoder_ladder":["h264_nvenc","h264_mf"]"#));
    assert!(s.contains(r#""crf":23"#));
    assert_eq!(serde_json::from_str::<RequestBody>(&s).unwrap(), transcode);
    assert_eq!(transcode.item_id(), None);
    assert_eq!(transcode.input_fingerprint(), None);
}

#[test]
fn video_frames_roundtrip_and_tags() {
    let cover = RequestBody::VideoFrames {
        session_id: 31,
        source_path: "D:/videos/a.mkv".into(),
        input_fingerprint: "fp-cover".into(),
        mode: VideoFramesMode::Cover { max_long_edge: 480 },
    };
    let s = serde_json::to_string(&cover).unwrap();
    assert!(s.contains(r#""op":"video_frames""#));
    assert!(s.contains(r#""kind":"cover""#));
    assert_eq!(serde_json::from_str::<RequestBody>(&s).unwrap(), cover);

    let keyframes = RequestBody::VideoFrames {
        session_id: 31,
        source_path: "D:/videos/a.mkv".into(),
        input_fingerprint: "fp-kf".into(),
        mode: VideoFramesMode::Keyframes {
            n: 9,
            cell_height: 120,
        },
    };
    let s = serde_json::to_string(&keyframes).unwrap();
    assert!(s.contains(r#""kind":"keyframes""#));
    assert!(s.contains(r#""n":9"#));
    assert!(s.contains(r#""cell_height":120"#));
    assert_eq!(serde_json::from_str::<RequestBody>(&s).unwrap(), keyframes);
}

#[test]
fn embed_result_status_tags() {
    let ok = EmbedResult::Ok {
        item_id: 1,
        fingerprint: "f1".into(),
    };
    let err = EmbedResult::Err {
        item_id: 2,
        fingerprint: "f2".into(),
        code: WorkerErrorCode::GpuUnavailable,
    };
    let s = serde_json::to_string(&vec![ok.clone(), err.clone()]).unwrap();
    assert!(s.contains(r#""status":"ok""#));
    assert!(s.contains(r#""status":"err""#));
    assert!(s.contains(r#""code":"gpu_unavailable""#));
    let back: Vec<EmbedResult> = serde_json::from_str(&s).unwrap();
    assert_eq!(back, vec![ok, err]);
}

#[test]
fn v2_error_codes_serde_and_retryable() {
    assert_eq!(WorkerErrorCode::GpuUnavailable.as_str(), "gpu_unavailable");
    assert_eq!(WorkerErrorCode::SessionExpired.as_str(), "session_expired");
    assert_eq!(
        WorkerErrorCode::ModelLoadFailed.as_str(),
        "model_load_failed"
    );
    assert_eq!(
        WorkerErrorCode::EmbedDimMismatch.as_str(),
        "embed_dim_mismatch"
    );
    assert!(WorkerErrorCode::GpuUnavailable.default_retryable());
    assert!(WorkerErrorCode::SessionExpired.default_retryable());
    assert!(!WorkerErrorCode::ModelLoadFailed.default_retryable());
    assert!(!WorkerErrorCode::EmbedDimMismatch.default_retryable());
    assert_eq!(
        serde_json::to_string(&WorkerErrorCode::SessionExpired).unwrap(),
        r#""session_expired""#
    );
}

#[test]
fn v3_error_codes_serde_and_retryable() {
    // 加固批 A:三个阶段化装载错误码,全部 terminal(重试同一 worker 无意义,病灶在环境)。
    assert_eq!(
        WorkerErrorCode::OrtDylibUnavailable.as_str(),
        "ort_dylib_unavailable"
    );
    assert_eq!(
        WorkerErrorCode::OrtRuntimeInitTimeout.as_str(),
        "ort_runtime_init_timeout"
    );
    assert_eq!(
        WorkerErrorCode::SessionLoadTimeout.as_str(),
        "session_load_timeout"
    );
    assert!(!WorkerErrorCode::OrtDylibUnavailable.default_retryable());
    assert!(!WorkerErrorCode::OrtRuntimeInitTimeout.default_retryable());
    assert!(!WorkerErrorCode::SessionLoadTimeout.default_retryable());
    assert_eq!(
        serde_json::to_string(&WorkerErrorCode::OrtDylibUnavailable).unwrap(),
        r#""ort_dylib_unavailable""#
    );
    let back: WorkerErrorCode = serde_json::from_str(r#""session_load_timeout""#).unwrap();
    assert_eq!(back, WorkerErrorCode::SessionLoadTimeout);
}

#[test]
fn ffmpeg_unavailable_serde_and_retryable() {
    // 视频格式扩展子系统 design.md §2.3/§3.4:镜像 OrtDylibUnavailable 先例,terminal。
    assert_eq!(
        WorkerErrorCode::FfmpegUnavailable.as_str(),
        "ffmpeg_unavailable"
    );
    assert!(!WorkerErrorCode::FfmpegUnavailable.default_retryable());
    assert_eq!(
        serde_json::to_string(&WorkerErrorCode::FfmpegUnavailable).unwrap(),
        r#""ffmpeg_unavailable""#
    );
    let back: WorkerErrorCode = serde_json::from_str(r#""ffmpeg_unavailable""#).unwrap();
    assert_eq!(back, WorkerErrorCode::FfmpegUnavailable);
}

#[test]
fn failure_body_optional_item_fields() {
    // 整批失败:无单项字段;缺省字段可解析(Option 特化)。
    let legacy = r#"{"code":"session_expired","retryable":true,"message":"m"}"#;
    let back: FailureBody = serde_json::from_str(legacy).unwrap();
    assert_eq!(back.item_id, None);
    assert_eq!(back.input_fingerprint, None);
    assert_eq!(back.code, WorkerErrorCode::SessionExpired);
}

#[test]
fn capability_names_stable() {
    assert_eq!(capability::THUMBNAIL, "thumbnail");
    assert_eq!(capability::METADATA, "metadata");
    assert_eq!(capability::EMBEDDING, "embedding");
    assert_eq!(capability::FACE_DETECT_EMBED, "face_detect_embed");
    assert_eq!(capability::OCR_TEXT, "ocr_text");
    assert_eq!(capability::ENHANCE, "enhance");
    assert_eq!(capability::VIDEO_PROBE, "video_probe");
    assert_eq!(capability::VIDEO_REMUX, "video_remux");
    assert_eq!(capability::VIDEO_TRANSCODE, "video_transcode");
    assert_eq!(capability::VIDEO_FRAMES, "video_frames");
}
