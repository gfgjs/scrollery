// src-tauri/src/ai/worker_client/tests.rs
//! D 组:worker_client 单测(原 `#[cfg(test)] mod tests { .. }` 原样搬入)。

use super::*;
use exotic_protocol::{EmbedBatchSuccess, EmbedResult, SessionReadyBody, SuccessBody};
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
                RawOutcome::TimedOut
                    | RawOutcome::Cancelled
                    | RawOutcome::Disconnected
                    | RawOutcome::Protocol(_)
            ) {
                self.alive = false;
                self.session = None;
            }
            return out;
        }
        let RequestBody::SessionInit {
            session_id,
            model_profile,
            models,
            ..
        } = req
        else {
            return RawOutcome::Protocol("非 SessionInit".into());
        };
        // 镜像真实 worker:载入 face 角色时回报 face_embed_dim(测试用 dim=2)。
        let face_dim = model_profile.face_profile_id.as_ref().map(|_| 2u32);
        let mut caps = Vec::new();
        for model in models {
            let cap = match model.role {
                ModelRole::ImageEncoder => capability::EMBEDDING,
                ModelRole::TextEncoder => capability::TEXT_ENCODING,
                ModelRole::FaceDetect => capability::FACE_DETECT_EMBED,
                _ => continue,
            };
            caps.push(cap.to_string());
        }
        self.session = Some(SessionDescriptor {
            session_id: *session_id,
            arch_id: model_profile.arch_id.clone(),
            image_file: model_profile.image_file.clone(),
            face_profile_id: model_profile.face_profile_id.clone(),
            batch_size: model_profile.batch_size,
            embed_dim: 2,
            face_embed_dim: face_dim,
            caps: caps.clone(),
            provider: Some("mock".into()),
            gpu_name: Some(String::new()),
        });
        RawOutcome::Success {
            body: SuccessBody {
                session: Some(SessionReadyBody {
                    embed_dim: 2,
                    face_embed_dim: face_dim,
                    caps,
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
            RawOutcome::TimedOut
                | RawOutcome::Cancelled
                | RawOutcome::Disconnected
                | RawOutcome::Protocol(_)
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
            cache_path: None,
            fingerprint: "f1".into(),
        },
        EmbedItem {
            item_id: 2,
            cache_key: "aaa2".into(),
            cache_path: None,
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
        roles: ClipRoles {
            image: true,
            text: true,
        },
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
fn cancelled_batch_returns_cancelled_without_spawning_worker() {
    let dir = temp_dir("cancelled");
    let spec = test_spec(&dir);
    let spawn = Arc::new(AtomicUsize::new(0));
    let mut client = client_with_script(
        vec![vec![ok_batch()]],
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
        Arc::clone(&spawn),
    );
    let outcome = client.embed_batch(&spec, &items2(), &|| true);
    assert!(matches!(outcome, Err(AppError::Cancelled)));
    assert_eq!(spawn.load(Ordering::SeqCst), 0);
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
