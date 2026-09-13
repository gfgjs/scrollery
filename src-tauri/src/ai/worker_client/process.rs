// src-tauri/src/ai/worker_client/process.rs
//! A 组:worker 子进程生命周期(spawn/存活探测/体面弃用/exe 路径解析)。

use super::*;

impl AiWorkerClient {
    /// 运行时构造:spawner = 真实子进程(exe 按 [`ai_worker_exe`] 解析)。
    pub fn new() -> Self {
        Self::with_spawner(Box::new(spawn_ai_worker))
    }

    /// 测试构造:注入 mock spawner。
    pub fn with_spawner(spawner: Spawner) -> Self {
        AiWorkerClient {
            worker: None,
            spawner,
            sha_cache: HashMap::new(),
            next_session_id: 1,
            ocr_loaded: None,
        }
    }

    /// 当前在载会话快照(T16:provider 回声落库/状态命令「已加载」判定消费);
    /// 无存活实例或无会话为 None。
    pub fn session(&self) -> Option<&SessionDescriptor> {
        self.worker
            .as_ref()
            .filter(|w| w.is_alive())
            .and_then(|w| w.session())
    }

    /// 弃用当前实例(体面退出;已死实例内部直接回收)。
    pub(super) fn drop_worker(&mut self) {
        if let Some(w) = self.worker.take() {
            w.shutdown(SHUTDOWN_GRACE);
        }
        // OCR 会话随进程消亡(失效点①):换代后必须重 init(D-OCR-1)。
        self.ocr_loaded = None;
    }

    /// 确保有存活 worker 实例(死亡/缺失即重建)。
    pub(super) fn ensure_worker(&mut self) -> Result<()> {
        if self.worker.as_ref().is_some_and(|w| w.is_alive()) {
            return Ok(());
        }
        self.drop_worker();
        let w = (self.spawner)()
            .map_err(|e| AppError::internal("AI worker 启动失败 | startup failed", e))?;
        info!("AI worker 已启动(version={})", w.worker_version());
        self.worker = Some(w);
        // 新进程无任何会话(失效点②:drop_worker 已复位,此处对「首次 spawn」路径兜底)。
        self.ocr_loaded = None;
        Ok(())
    }
}

/// 解析 ai-worker 可执行文件:环境变量 `PICASA_AI_WORKER_PATH` 覆盖(开发/测试),
/// 缺省取主程序同目录(workspace 构建与打包分发的共同布局;签名产线随 Part7)。
pub fn ai_worker_exe() -> std::result::Result<PathBuf, String> {
    if let Ok(p) = std::env::var("PICASA_AI_WORKER_PATH") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Ok(p);
        }
        return Err(format!(
            "PICASA_AI_WORKER_PATH 指向的文件不存在:{}",
            p.display()
        ));
    }
    let exe = std::env::current_exe().map_err(|e| format!("current_exe 失败:{e}"))?;
    let dir = exe.parent().ok_or("current_exe 无父目录")?;
    let p = dir.join(format!("ai-worker{}", std::env::consts::EXE_SUFFIX));
    if p.is_file() {
        Ok(p)
    } else {
        Err(format!("ai-worker 可执行文件不存在:{}", p.display()))
    }
}

/// 真实 spawner:解析 exe → WorkerSupervisor::spawn(握手校验 worker_id/能力)。
fn spawn_ai_worker() -> std::result::Result<Box<dyn EmbedWorker>, String> {
    let spec = WorkerSpec {
        exe_path: ai_worker_exe()?,
        expected_worker_id: "ai-worker".to_string(),
        required_capabilities: vec![capability::EMBEDDING.to_string()],
    };
    let cfg = WorkerConfig {
        handshake_timeout: HANDSHAKE_TIMEOUT,
        host_version: env!("CARGO_PKG_VERSION").to_string(),
        max_blob_len: MAX_BLOB_LEN,
    };
    WorkerSupervisor::spawn(&spec, &cfg).map(|s| Box::new(s) as Box<dyn EmbedWorker>)
}
