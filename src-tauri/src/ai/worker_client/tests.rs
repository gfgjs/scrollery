// src-tauri/src/ai/worker_client/tests.rs
//! D 组:worker_client 单测(原 `#[cfg(test)] mod tests { .. }` 原样搬入)。

use super::session::build_session_init;
use super::*;
use exotic_protocol::{
    EmbedBatchSuccess, EmbedResult, FaceBatchSuccess, FaceItemResult, FailureBody,
    SessionReadyBody, SuccessBody, WorkerErrorCode,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// 脚本化 mock worker:记录调用次数,按预设应答批请求。
struct MockEmbedWorker {
    alive: bool,
    session: Option<SessionDescriptor>,
    /// 每次 init_session 先从此弹应答;耗尽后走默认成功路径。用于模拟
    /// 「alive 旗标陈旧 → init 写死管道」(W1)等 init 期异常。
    init_script: Vec<RawOutcome>,
    /// 每次 run_batch 依序弹出一个应答;耗尽后回 Disconnected。
    batch_script: Vec<RawOutcome>,
    init_count: Arc<AtomicUsize>,
    close_count: Arc<AtomicUsize>,
}

impl crate::exotic::pipeline::WorkerTask for MockEmbedWorker {
    fn worker_version(&self) -> String {
        "mock".into()
    }
    fn is_alive(&self) -> bool {
        self.alive
    }
    fn shutdown(self: Box<Self>, _grace: Duration) {}
}

impl EmbedWorker for MockEmbedWorker {
    fn session(&self) -> Option<&SessionDescriptor> {
        self.session.as_ref()
    }
    fn init_session(
        &mut self,
        req: &RequestBody,
        _timeout: Duration,
        _cancelled: &dyn Fn() -> bool,
    ) -> RawOutcome {
        self.init_count.fetch_add(1, Ordering::SeqCst);
        if !self.init_script.is_empty() {
            let out = self.init_script.remove(0);
            // 进程级异常语义与 run_batch 一致:真实 Supervisor 会 kill 标死。
            if matches!(
                out,
                RawOutcome::TimedOut | RawOutcome::Disconnected | RawOutcome::Protocol(_)
            ) {
                self.alive = false;
                self.session = None;
            }
            return out;
        }
        let RequestBody::SessionInit {
            session_id,
            model_profile,
            ..
        } = req
        else {
            return RawOutcome::Protocol("非 SessionInit".into());
        };
        // 镜像真实 worker:载入 face 角色时回报 face_embed_dim(测试用 dim=2)。
        let face_dim = model_profile.face_profile_id.as_ref().map(|_| 2u32);
        self.session = Some(SessionDescriptor {
            session_id: *session_id,
            arch_id: model_profile.arch_id.clone(),
            image_file: model_profile.image_file.clone(),
            face_profile_id: model_profile.face_profile_id.clone(),
            batch_size: model_profile.batch_size,
            embed_dim: 2,
            face_embed_dim: face_dim,
            caps: vec!["embedding".into()],
            provider: Some("mock".into()),
            gpu_name: Some(String::new()),
        });
        RawOutcome::Success {
            body: SuccessBody {
                session: Some(SessionReadyBody {
                    embed_dim: 2,
                    face_embed_dim: face_dim,
                    caps: vec!["embedding".into()],
                    provider: Some("mock".into()),
                    gpu_name: Some(String::new()),
                }),
                ..Default::default()
            },
            blob: Vec::new(),
        }
    }
    fn close_session(&mut self, _timeout: Duration, _cancelled: &dyn Fn() -> bool) -> RawOutcome {
        self.close_count.fetch_add(1, Ordering::SeqCst);
        self.session = None;
        RawOutcome::Success {
            body: SuccessBody::default(),
            blob: Vec::new(),
        }
    }
    fn run_batch(
        &mut self,
        _req: &RequestBody,
        _timeout: Duration,
        _cancelled: &dyn Fn() -> bool,
    ) -> RawOutcome {
        if self.batch_script.is_empty() {
            self.alive = false;
            return RawOutcome::Disconnected;
        }
        let out = self.batch_script.remove(0);
        // 进程级异常语义:真实 Supervisor 会 kill 标死,mock 同步。
        if matches!(
            out,
            RawOutcome::TimedOut | RawOutcome::Disconnected | RawOutcome::Protocol(_)
        ) {
            self.alive = false;
            self.session = None;
        }
        out
    }
}

/// 一个 dim=2、双项全 Ok 的合法批应答。
fn ok_batch() -> RawOutcome {
    let mut blob = Vec::new();
    for f in [1.0f32, 2.0, 3.0, 4.0] {
        blob.extend_from_slice(&f.to_le_bytes());
    }
    RawOutcome::Success {
        body: SuccessBody {
            embed: Some(EmbedBatchSuccess {
                results: vec![
                    EmbedResult::Ok {
                        item_id: 1,
                        fingerprint: "f1".into(),
                    },
                    EmbedResult::Ok {
                        item_id: 2,
                        fingerprint: "f2".into(),
                    },
                ],
            }),
            ..Default::default()
        },
        blob,
    }
}

fn items2() -> Vec<EmbedItem> {
    vec![
        EmbedItem {
            item_id: 1,
            cache_key: "aaa1".into(),
            fingerprint: "f1".into(),
        },
        EmbedItem {
            item_id: 2,
            cache_key: "aaa2".into(),
            fingerprint: "f2".into(),
        },
    ]
}

/// 测试规格:profile 用注册表默认,模型文件不落盘(mock 不校验载荷,
/// build_session_init 也不会被 mock 路径拒——sha 计算需要真文件,因此测试用
/// 临时目录铺两份契约文件)。
fn test_spec(dir: &Path) -> SessionSpec {
    let profile = crate::ai::profile::default_profile();
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(&profile.image_file), b"img").unwrap();
    std::fs::write(dir.join(&profile.text_file), b"txt").unwrap();
    SessionSpec {
        profile,
        face_profile: None,
        models_dir: dir.to_path_buf(),
        ai_cache_dir: dir.join("ai_thumbs"),
        image_provider: "cpu".into(),
        batch_size: 16,
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ai-worker-client-test-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn client_with_script(
    scripts: Vec<Vec<RawOutcome>>,
    init_count: Arc<AtomicUsize>,
    close_count: Arc<AtomicUsize>,
    spawn_count: Arc<AtomicUsize>,
) -> AiWorkerClient {
    let scripts = std::sync::Mutex::new(scripts);
    AiWorkerClient::with_spawner(Box::new(move || {
        spawn_count.fetch_add(1, Ordering::SeqCst);
        let mut s = scripts.lock().unwrap();
        if s.is_empty() {
            return Err("mock spawner 脚本耗尽".into());
        }
        Ok(Box::new(MockEmbedWorker {
            alive: true,
            session: None,
            init_script: Vec::new(),
            batch_script: s.remove(0),
            init_count: Arc::clone(&init_count),
            close_count: Arc::clone(&close_count),
        }) as Box<dyn EmbedWorker>)
    }))
}

#[test]
fn embed_batch_happy_path_inits_session_once() {
    let dir = temp_dir("happy");
    let spec = test_spec(&dir);
    let init = Arc::new(AtomicUsize::new(0));
    let close = Arc::new(AtomicUsize::new(0));
    let spawn = Arc::new(AtomicUsize::new(0));
    let mut c = client_with_script(
        vec![vec![ok_batch(), ok_batch()]],
        Arc::clone(&init),
        Arc::clone(&close),
        Arc::clone(&spawn),
    );

    let out = c.embed_batch(&spec, &items2(), &|| false).unwrap();
    assert_eq!(out.len(), 2);
    assert!(matches!(&out[0], EmbedItemOutcome::Ok(v) if v == &vec![1.0, 2.0]));
    // 第二批复用同一会话:init 不再发生。
    let _ = c.embed_batch(&spec, &items2(), &|| false).unwrap();
    assert_eq!(init.load(Ordering::SeqCst), 1, "会话快照匹配应零帧复用");
    assert_eq!(spawn.load(Ordering::SeqCst), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn embed_batch_respawns_after_timeout_then_succeeds() {
    let dir = temp_dir("timeout-retry");
    let spec = test_spec(&dir);
    let init = Arc::new(AtomicUsize::new(0));
    let close = Arc::new(AtomicUsize::new(0));
    let spawn = Arc::new(AtomicUsize::new(0));
    // 第一个实例:超时(死);第二个实例:成功。
    let mut c = client_with_script(
        vec![vec![RawOutcome::TimedOut], vec![ok_batch()]],
        Arc::clone(&init),
        Arc::clone(&close),
        Arc::clone(&spawn),
    );

    let out = c.embed_batch(&spec, &items2(), &|| false).unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(spawn.load(Ordering::SeqCst), 2, "超时后应重建实例");
    assert_eq!(init.load(Ordering::SeqCst), 2, "重建后应重建会话");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn embed_batch_hard_stop_after_two_failures() {
    let dir = temp_dir("hard-stop");
    let spec = test_spec(&dir);
    let init = Arc::new(AtomicUsize::new(0));
    let close = Arc::new(AtomicUsize::new(0));
    let spawn = Arc::new(AtomicUsize::new(0));
    // 两个实例都超时 → 硬止损返错;第三个实例不该被创建。
    let mut c = client_with_script(
        vec![
            vec![RawOutcome::TimedOut],
            vec![RawOutcome::TimedOut],
            vec![ok_batch()],
        ],
        Arc::clone(&init),
        Arc::clone(&close),
        Arc::clone(&spawn),
    );
    assert!(c.embed_batch(&spec, &items2(), &|| false).is_err());
    assert_eq!(spawn.load(Ordering::SeqCst), 2, "硬止损:不应第三次重建");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_expired_reinits_same_instance() {
    let dir = temp_dir("expired");
    let spec = test_spec(&dir);
    let init = Arc::new(AtomicUsize::new(0));
    let close = Arc::new(AtomicUsize::new(0));
    let spawn = Arc::new(AtomicUsize::new(0));
    let expired = RawOutcome::Failure(FailureBody {
        item_id: None,
        input_fingerprint: None,
        code: WorkerErrorCode::SessionExpired,
        retryable: true,
        message: "会话未加载".into(),
    });
    let mut c = client_with_script(
        vec![vec![expired, ok_batch()]],
        Arc::clone(&init),
        Arc::clone(&close),
        Arc::clone(&spawn),
    );
    let out = c.embed_batch(&spec, &items2(), &|| false).unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(spawn.load(Ordering::SeqCst), 1, "实例存活,不重建进程");
    assert_eq!(init.load(Ordering::SeqCst), 2, "会话失效应重 init");
    assert!(
        close.load(Ordering::SeqCst) >= 1,
        "重 init 前应先 close 清快照"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn terminal_failure_does_not_retry() {
    let dir = temp_dir("terminal");
    let spec = test_spec(&dir);
    let init = Arc::new(AtomicUsize::new(0));
    let close = Arc::new(AtomicUsize::new(0));
    let spawn = Arc::new(AtomicUsize::new(0));
    let dim_mismatch = RawOutcome::Failure(FailureBody {
        item_id: None,
        input_fingerprint: None,
        code: WorkerErrorCode::EmbedDimMismatch,
        retryable: false,
        message: "维度不符".into(),
    });
    let mut c = client_with_script(
        vec![vec![dim_mismatch, ok_batch()]],
        Arc::clone(&init),
        Arc::clone(&close),
        Arc::clone(&spawn),
    );
    let e = c.embed_batch(&spec, &items2(), &|| false).unwrap_err();
    assert!(e.to_string().contains("embed_dim_mismatch"), "err: {e}");
    assert_eq!(init.load(Ordering::SeqCst), 1, "terminal 失败不得重试");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_session_init_lists_clip_pair_with_integrity() {
    let dir = temp_dir("build-init");
    let spec = test_spec(&dir);
    let mut cache = HashMap::new();
    let req = build_session_init(7, &spec, &mut cache).unwrap();
    let RequestBody::SessionInit {
        session_id,
        models,
        model_profile,
        models_root,
        ..
    } = req
    else {
        panic!("应为 SessionInit");
    };
    assert_eq!(session_id, 7);
    assert_eq!(models.len(), 2, "无 face profile 时只列 CLIP 成对");
    let img = models
        .iter()
        .find(|m| m.role == ModelRole::ImageEncoder)
        .unwrap();
    assert_eq!(img.len, 3);
    assert_eq!(img.sha256, crate::utils::hash::sha256_hex(b"img"));
    assert!(matches!(&img.handle, ModelHandle::Path(p) if p.ends_with(&spec.profile.image_file)));
    assert_eq!(model_profile.arch_id, spec.profile.id);
    assert_eq!(model_profile.face_profile_id, None);
    assert_eq!(models_root, spec.models_dir.to_string_lossy());
    // sha 备忘:同文件未变更,二次组装命中缓存(条目仍在且值不变)。
    let req2 = build_session_init(8, &spec, &mut cache).unwrap();
    let RequestBody::SessionInit { models: m2, .. } = req2 else {
        panic!()
    };
    assert_eq!(m2[0].sha256, models[0].sha256);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn missing_model_file_maps_to_model_not_loaded() {
    let dir = temp_dir("missing-model");
    let spec = test_spec(&dir);
    std::fs::remove_file(dir.join(&spec.profile.image_file)).unwrap();
    let mut cache = HashMap::new();
    let e = build_session_init(1, &spec, &mut cache).unwrap_err();
    assert!(matches!(e, AppError::AiModelNotLoaded(_)), "err: {e}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// face 接线波用规格:在 test_spec 之上再铺 face 双 onnx 契约文件。
fn test_spec_with_face(dir: &Path) -> SessionSpec {
    let mut spec = test_spec(dir);
    let face = crate::ai::face_profile::default_face_profile();
    std::fs::write(dir.join(&face.detect_file), b"det").unwrap();
    std::fs::write(dir.join(&face.embed_file), b"emb").unwrap();
    spec.face_profile = Some(face);
    spec
}

#[test]
fn session_spec_superset_match() {
    let dir = temp_dir("superset");
    let plain = test_spec(&dir);
    let with_face = test_spec_with_face(&dir);
    let face_id = with_face.face_profile.as_ref().unwrap().id.clone();
    let desc = |face: Option<String>| SessionDescriptor {
        session_id: 1,
        arch_id: plain.profile.id.clone(),
        image_file: plain.profile.image_file.clone(),
        face_profile_id: face,
        batch_size: 16,
        embed_dim: 2,
        face_embed_dim: None,
        caps: vec![],
        provider: None,
        gpu_name: None,
    };
    // 超集放宽:不需要人脸的 spec 可复用带人脸的合并会话。
    assert!(plain.matches(&desc(Some(face_id.clone()))));
    assert!(plain.matches(&desc(None)));
    // 需要人脸的 spec 必须精确匹配:CLIP-only 会话不满足 → 切换。
    assert!(!with_face.matches(&desc(None)));
    assert!(with_face.matches(&desc(Some(face_id))));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn face_detect_embed_switches_session_and_validates_dims() {
    let dir = temp_dir("face-batch");
    let plain = test_spec(&dir);
    let with_face = test_spec_with_face(&dir);
    let init = Arc::new(AtomicUsize::new(0));
    let close = Arc::new(AtomicUsize::new(0));
    let spawn = Arc::new(AtomicUsize::new(0));

    let fitem = FaceItem {
        item_id: 1,
        cache_key: None,
        source_path: Some("x.webp".into()),
        fingerprint: "ff1".into(),
    };
    // 应答:1 项 Ok、1 张脸(dim=2 → blob 8 字节),带实际解码尺寸。
    let mut blob = Vec::new();
    for f in [0.1f32, 0.2] {
        blob.extend_from_slice(&f.to_le_bytes());
    }
    let face_ok = RawOutcome::Success {
        body: SuccessBody {
            face: Some(FaceBatchSuccess {
                results: vec![FaceItemResult::Ok {
                    item_id: 1,
                    fingerprint: "ff1".into(),
                    faces: vec![exotic_protocol::FaceDet {
                        bbox: [1.0, 2.0, 3.0, 4.0],
                        landmarks: [[0.0; 2]; 5],
                        score: 0.9,
                    }],
                    width: 640,
                    height: 480,
                }],
            }),
            ..Default::default()
        },
        blob,
    };
    let mut c = client_with_script(
        vec![vec![ok_batch(), face_ok]],
        Arc::clone(&init),
        Arc::clone(&close),
        Arc::clone(&spawn),
    );

    // 先建 CLIP-only 会话,再发 face 批 → 必须切换(close + 重 init 合并会话)。
    let _ = c.embed_batch(&plain, &items2(), &|| false).unwrap();
    let out = c
        .face_detect_embed(&with_face, &[fitem], 0.9, &|| false)
        .unwrap();
    assert_eq!(init.load(Ordering::SeqCst), 2, "face spec 应触发会话切换");
    assert!(close.load(Ordering::SeqCst) >= 1, "切换前应先 close 旧会话");
    assert_eq!(spawn.load(Ordering::SeqCst), 1, "切换不重建进程");
    match &out[0] {
        FaceItemOutcome::Ok {
            faces,
            embeddings,
            width,
            height,
        } => {
            assert_eq!(faces.len(), 1);
            assert_eq!(embeddings[0], vec![0.1, 0.2]);
            assert_eq!((*width, *height), (640, 480));
        }
        _ => panic!("期望 Ok"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// 同 client_with_script,但每实例带 init 脚本(W1 场景:init 期进程级异常)。
fn client_with_init_scripts(
    scripts: Vec<(Vec<RawOutcome>, Vec<RawOutcome>)>,
    init_count: Arc<AtomicUsize>,
    close_count: Arc<AtomicUsize>,
    spawn_count: Arc<AtomicUsize>,
) -> AiWorkerClient {
    let scripts = std::sync::Mutex::new(scripts);
    AiWorkerClient::with_spawner(Box::new(move || {
        spawn_count.fetch_add(1, Ordering::SeqCst);
        let mut s = scripts.lock().unwrap();
        if s.is_empty() {
            return Err("mock spawner 脚本耗尽".into());
        }
        let (init_script, batch_script) = s.remove(0);
        Ok(Box::new(MockEmbedWorker {
            alive: true,
            session: None,
            init_script,
            batch_script,
            init_count: Arc::clone(&init_count),
            close_count: Arc::clone(&close_count),
        }) as Box<dyn EmbedWorker>)
    }))
}

/// W1 回归:worker 空闲自杀后 host 旗标陈旧 → init 写死管道(Disconnected)。
/// 进程级 init 失败必须落入 attempt 圈重建重发,一次调用内自愈。
#[test]
fn stale_alive_flag_init_disconnect_recovers_within_one_call() {
    let dir = temp_dir("w1-stale");
    let spec = test_spec(&dir);
    let init = Arc::new(AtomicUsize::new(0));
    let close = Arc::new(AtomicUsize::new(0));
    let spawn = Arc::new(AtomicUsize::new(0));
    // 实例1:init 即断开(模拟死管道);实例2:正常。
    let mut c = client_with_init_scripts(
        vec![
            (vec![RawOutcome::Disconnected], vec![]),
            (vec![], vec![ok_batch()]),
        ],
        Arc::clone(&init),
        Arc::clone(&close),
        Arc::clone(&spawn),
    );
    let out = c.embed_batch(&spec, &items2(), &|| false).unwrap();
    assert_eq!(out.len(), 2, "第二次 attempt 应重建实例并成功");
    assert_eq!(spawn.load(Ordering::SeqCst), 2, "死管道后应重建进程");
    let _ = std::fs::remove_dir_all(&dir);
}

/// W1 边界:init 期数据级失败(worker 明确拒绝)不得消耗第二次 attempt。
#[test]
fn init_data_failure_bubbles_without_respawn() {
    let dir = temp_dir("w1-terminal");
    let spec = test_spec(&dir);
    let init = Arc::new(AtomicUsize::new(0));
    let close = Arc::new(AtomicUsize::new(0));
    let spawn = Arc::new(AtomicUsize::new(0));
    let load_fail = RawOutcome::Failure(FailureBody {
        item_id: None,
        input_fingerprint: None,
        code: WorkerErrorCode::ModelLoadFailed,
        retryable: false,
        message: "sha 不符".into(),
    });
    let mut c = client_with_init_scripts(
        vec![(vec![load_fail], vec![]), (vec![], vec![ok_batch()])],
        Arc::clone(&init),
        Arc::clone(&close),
        Arc::clone(&spawn),
    );
    let e = c.embed_batch(&spec, &items2(), &|| false).unwrap_err();
    assert!(e.to_string().contains("model_load_failed"), "err: {e}");
    assert_eq!(spawn.load(Ordering::SeqCst), 1, "数据级失败不得重建重试");
    let _ = std::fs::remove_dir_all(&dir);
}

/// W2 回归:retryable 失败(瞬态推理错误)应在同实例同会话上重发一次并成功。
#[test]
fn retryable_failure_retries_same_instance_and_session() {
    let dir = temp_dir("w2-retryable");
    let spec = test_spec(&dir);
    let init = Arc::new(AtomicUsize::new(0));
    let close = Arc::new(AtomicUsize::new(0));
    let spawn = Arc::new(AtomicUsize::new(0));
    let transient = RawOutcome::Failure(FailureBody {
        item_id: None,
        input_fingerprint: None,
        code: WorkerErrorCode::InternalError,
        retryable: true,
        message: "DirectML 瞬态错误".into(),
    });
    let mut c = client_with_script(
        vec![vec![transient, ok_batch()]],
        Arc::clone(&init),
        Arc::clone(&close),
        Arc::clone(&spawn),
    );
    let out = c.embed_batch(&spec, &items2(), &|| false).unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(spawn.load(Ordering::SeqCst), 1, "瞬态失败不重建进程");
    assert_eq!(init.load(Ordering::SeqCst), 1, "瞬态失败不重建会话");
    let _ = std::fs::remove_dir_all(&dir);
}

/// W3 回归:调大 batch 后旧会话(上限更小)必须切换重建;调小则复用。
#[test]
fn batch_size_grow_forces_switch_shrink_reuses() {
    let dir = temp_dir("w3-batch");
    let spec16 = test_spec(&dir);
    let init = Arc::new(AtomicUsize::new(0));
    let close = Arc::new(AtomicUsize::new(0));
    let spawn = Arc::new(AtomicUsize::new(0));
    let mut c = client_with_script(
        vec![vec![ok_batch(), ok_batch(), ok_batch()]],
        Arc::clone(&init),
        Arc::clone(&close),
        Arc::clone(&spawn),
    );
    let _ = c.embed_batch(&spec16, &items2(), &|| false).unwrap();
    assert_eq!(init.load(Ordering::SeqCst), 1);

    // 调大:desc.batch_size(16)< spec(32)→ 切换重 init。
    let mut spec32 = spec16.clone();
    spec32.batch_size = 32;
    let _ = c.embed_batch(&spec32, &items2(), &|| false).unwrap();
    assert_eq!(init.load(Ordering::SeqCst), 2, "批上限增大应重建会话");
    assert!(close.load(Ordering::SeqCst) >= 1, "切换前应先 close");

    // 调小:desc(32)≥ spec(8)→ 复用,零帧。
    let mut spec8 = spec16.clone();
    spec8.batch_size = 8;
    let _ = c.embed_batch(&spec8, &items2(), &|| false).unwrap();
    assert_eq!(init.load(Ordering::SeqCst), 2, "批上限缩小应复用会话");
    assert_eq!(spawn.load(Ordering::SeqCst), 1, "全程不重建进程");
    let _ = std::fs::remove_dir_all(&dir);
}

// ── OCR 会话/批 attempt 圈(T6)──────────────────────────────────────────────────
//
// 复用 client_with_script:OCR 的 OcrSessionInit 与 OcrBatch 都走 EmbedWorker::run_batch
// (mock 只按序弹 batch_script),故一条脚本内 init 应答与 batch 应答同列。

/// OcrSessionInit 的合法应答(带 ocr_session)。
fn ocr_init_ok() -> RawOutcome {
    RawOutcome::Success {
        body: SuccessBody {
            ocr_session: Some(exotic_protocol::OcrSessionReadyBody {
                caps: vec![capability::OCR_TEXT.into()],
            }),
            ..Default::default()
        },
        blob: Vec::new(),
    }
}

/// 单项 Ok 的合法 OcrBatch 应答(blob 恒空)。
fn ocr_batch_ok() -> RawOutcome {
    RawOutcome::Success {
        body: SuccessBody {
            ocr: Some(exotic_protocol::OcrBatchSuccess {
                results: vec![exotic_protocol::OcrItemResult::Ok {
                    item_id: 1,
                    fingerprint: "of1".into(),
                    lines: vec![exotic_protocol::OcrLine {
                        text: "hi".into(),
                        quad: [[0.0, 0.0], [8.0, 0.0], [8.0, 4.0], [0.0, 4.0]],
                        confidence: 0.9,
                    }],
                    width: 100,
                    height: 50,
                }],
            }),
            ..Default::default()
        },
        blob: Vec::new(),
    }
}

fn ocr_item1() -> OcrItem {
    OcrItem {
        item_id: 1,
        cache_key: None,
        source_path: Some("i.png".into()),
        fingerprint: "of1".into(),
    }
}

/// OCR 测试规格:铺 profile 四文件契约(sha 计算需真文件)。
fn test_ocr_spec(dir: &Path) -> OcrSessionSpec {
    let profile = scrollery_ai_core::ocr_profile::default_ocr_profile();
    std::fs::create_dir_all(dir).unwrap();
    for f in [
        &profile.det_file,
        &profile.cls_file,
        &profile.rec_file,
        &profile.dict_file,
    ] {
        std::fs::write(dir.join(f), b"m").unwrap();
    }
    OcrSessionSpec {
        profile,
        models_dir: dir.to_path_buf(),
    }
}

#[test]
fn ocr_batch_happy_path_inits_session_once() {
    let dir = temp_dir("ocr-happy");
    let spec = test_ocr_spec(&dir);
    let init = Arc::new(AtomicUsize::new(0));
    let close = Arc::new(AtomicUsize::new(0));
    let spawn = Arc::new(AtomicUsize::new(0));
    // 脚本:init + batch + batch。第二 batch 复用会话时才够用;若误重 init,会把
    // ocr_batch_ok 当 init 应答(缺 ocr_session)→ Terminal 错 → .unwrap 崩,反证复用。
    let mut c = client_with_script(
        vec![vec![ocr_init_ok(), ocr_batch_ok(), ocr_batch_ok()]],
        Arc::clone(&init),
        Arc::clone(&close),
        Arc::clone(&spawn),
    );
    let out = c.ocr_batch(&spec, &[ocr_item1()], &|| false).unwrap();
    assert_eq!(out.len(), 1);
    assert!(matches!(&out[0], OcrItemOutcome::Ok { lines, .. } if lines.len() == 1));
    // 第二批复用 OCR 会话:零帧 init。
    let _ = c.ocr_batch(&spec, &[ocr_item1()], &|| false).unwrap();
    assert_eq!(
        spawn.load(Ordering::SeqCst),
        1,
        "同 profile 应复用 OCR 会话"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ocr_batch_respawn_resets_loaded_and_reinits() {
    let dir = temp_dir("ocr-respawn");
    let spec = test_ocr_spec(&dir);
    let init = Arc::new(AtomicUsize::new(0));
    let close = Arc::new(AtomicUsize::new(0));
    let spawn = Arc::new(AtomicUsize::new(0));
    // 实例1:init+batch(首 call 成功),脚本随即耗尽;实例2:init+batch。
    let mut c = client_with_script(
        vec![
            vec![ocr_init_ok(), ocr_batch_ok()],
            vec![ocr_init_ok(), ocr_batch_ok()],
        ],
        Arc::clone(&init),
        Arc::clone(&close),
        Arc::clone(&spawn),
    );
    let _ = c.ocr_batch(&spec, &[ocr_item1()], &|| false).unwrap();
    assert_eq!(spawn.load(Ordering::SeqCst), 1);
    // 第二 call:实例1 脚本耗尽 → run_batch 回 Disconnected(死)→ 换代实例2 →
    // ocr_loaded 经 drop_worker 复位 → 重 init 重 batch,一次调用内自愈。
    let out = c.ocr_batch(&spec, &[ocr_item1()], &|| false).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(
        spawn.load(Ordering::SeqCst),
        2,
        "worker 换代应重建进程并重 init"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ocr_batch_session_expired_reinits_same_instance() {
    let dir = temp_dir("ocr-expired");
    let spec = test_ocr_spec(&dir);
    let init = Arc::new(AtomicUsize::new(0));
    let close = Arc::new(AtomicUsize::new(0));
    let spawn = Arc::new(AtomicUsize::new(0));
    let expired = RawOutcome::Failure(FailureBody {
        item_id: None,
        input_fingerprint: None,
        code: WorkerErrorCode::SessionExpired,
        retryable: true,
        message: "OCR 会话未加载".into(),
    });
    // 一实例:init, batch→SessionExpired, 重 init, batch_ok。
    let mut c = client_with_script(
        vec![vec![ocr_init_ok(), expired, ocr_init_ok(), ocr_batch_ok()]],
        Arc::clone(&init),
        Arc::clone(&close),
        Arc::clone(&spawn),
    );
    let out = c.ocr_batch(&spec, &[ocr_item1()], &|| false).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(spawn.load(Ordering::SeqCst), 1, "会话失效不重建进程");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ocr_batch_validation_violation_respawns() {
    let dir = temp_dir("ocr-badval");
    let spec = test_ocr_spec(&dir);
    let init = Arc::new(AtomicUsize::new(0));
    let close = Arc::new(AtomicUsize::new(0));
    let spawn = Arc::new(AtomicUsize::new(0));
    // 违例 batch:item_id 错位(999 != 1)→ validate 拒 → 弃实例。
    let bad = RawOutcome::Success {
        body: SuccessBody {
            ocr: Some(exotic_protocol::OcrBatchSuccess {
                results: vec![exotic_protocol::OcrItemResult::Ok {
                    item_id: 999,
                    fingerprint: "of1".into(),
                    lines: vec![],
                    width: 10,
                    height: 10,
                }],
            }),
            ..Default::default()
        },
        blob: Vec::new(),
    };
    let mut c = client_with_script(
        vec![
            vec![ocr_init_ok(), bad],
            vec![ocr_init_ok(), ocr_batch_ok()],
        ],
        Arc::clone(&init),
        Arc::clone(&close),
        Arc::clone(&spawn),
    );
    let out = c.ocr_batch(&spec, &[ocr_item1()], &|| false).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(spawn.load(Ordering::SeqCst), 2, "输出违例应弃实例重建");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 2026-07-23 深审#1:OcrSessionInit 撞 `ModelLoadFailed` 须类型化映射为
/// `AppError::Ocr{code:"ocr_model_missing"}`(而非笼统 `AppError::System` 字符串),
/// 供命令层 `ocr_commands::run_ocr_batch` 直接透传、前端按 code 分流引导重下载。
#[test]
fn ocr_session_init_model_load_failed_maps_to_model_missing() {
    let dir = temp_dir("ocr-model-missing");
    let spec = test_ocr_spec(&dir);
    let init = Arc::new(AtomicUsize::new(0));
    let close = Arc::new(AtomicUsize::new(0));
    let spawn = Arc::new(AtomicUsize::new(0));
    let load_fail = RawOutcome::Failure(FailureBody {
        item_id: None,
        input_fingerprint: None,
        code: WorkerErrorCode::ModelLoadFailed,
        retryable: false,
        message: "sha 不符".into(),
    });
    let mut c = client_with_script(
        vec![vec![load_fail]],
        Arc::clone(&init),
        Arc::clone(&close),
        Arc::clone(&spawn),
    );
    let e = c.ocr_batch(&spec, &[ocr_item1()], &|| false).unwrap_err();
    match e {
        AppError::Ocr { code, .. } => assert_eq!(code, "ocr_model_missing"),
        other => panic!("期望 AppError::Ocr{{code:\"ocr_model_missing\"}},实得: {other}"),
    }
    assert_eq!(
        spawn.load(Ordering::SeqCst),
        1,
        "terminal 数据级失败不得重建重试"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// 版本演进韧性:build_ocr_session_spec 对已下线/改名档位不硬错,回退默认。
/// build_ocr_session_spec 需 AppState(重),此处直测其依赖的解析表达式(单源同构)。
#[test]
fn unknown_ocr_tier_falls_back_to_default() {
    use scrollery_ai_core::ocr_profile::{
        default_ocr_profile, find_ocr_profile, DEFAULT_OCR_PROFILE_ID,
    };
    let resolved = find_ocr_profile("已下线的档位-v0").unwrap_or_else(default_ocr_profile);
    assert_eq!(resolved.id, DEFAULT_OCR_PROFILE_ID);
    // 合法档位仍精确解析,不被回退掩盖。
    let ok = find_ocr_profile("pp-ocrv5-server").unwrap_or_else(default_ocr_profile);
    assert_eq!(ok.id, "pp-ocrv5-server");
}
