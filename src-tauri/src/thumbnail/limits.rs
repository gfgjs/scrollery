//! 缩略图启动期资源设置；宿主向隔离 worker 传递同一快照，修改后重启生效。

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// 同一次应用启动中固定的缩略图资源上限。
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThumbnailLimits {
    pub fast_threads: usize,
    pub tail_threads: usize,
    pub gpu_inflight: usize,
    pub gpu_per_adapter: usize,
    pub process_memory_mb: usize,
    pub total_memory_mb: usize,
    pub workset_mb: usize,
}

impl ThumbnailLimits {
    /// 从配置读取；缺省值使用设置 schema，避免运行端与设置页各自维护默认值。
    pub fn from_reader(read: impl Fn(&str) -> Option<String>) -> Self {
        let number = |key| {
            let def = crate::config::schema::SettingDef::find(key).expect("thumbnail setting");
            read(key)
                .as_deref()
                .unwrap_or(def.default)
                .parse()
                .expect("validated setting")
        };
        Self {
            fast_threads: number("thumb_fast_threads"),
            tail_threads: number("thumb_tail_threads"),
            gpu_inflight: number("thumb_gpu_inflight"),
            gpu_per_adapter: number("thumb_gpu_per_adapter"),
            process_memory_mb: number("thumb_process_memory_mb"),
            total_memory_mb: number("thumb_total_memory_mb"),
            workset_mb: number("thumb_workset_mb"),
        }
    }

    /// 自动档从 CPU 预算扣除重型线程，手动档直接采用用户值。
    pub fn fast_threads_for(self, cpu_budget: usize) -> usize {
        if self.fast_threads == 0 {
            cpu_budget.saturating_sub(self.tail_threads).clamp(1, 64)
        } else {
            self.fast_threads
        }
    }

    pub(crate) fn process_memory_bytes(self) -> usize {
        self.process_memory_mb * 1024 * 1024
    }

    /// 固定可创建图像 GPU 会话的线程，避免任务轮转让所有 CPU 线程各留一份会话。
    #[cfg(windows)]
    pub fn gpu_threads_for(self, capacity: usize) -> usize {
        let adapters =
            crate::video::d3d::hardware_adapters().map_or(1, |adapters| adapters.len().max(1));
        capacity
            .min(self.gpu_inflight)
            .min(self.gpu_per_adapter.saturating_mul(adapters))
    }

    pub(crate) fn total_memory_bytes(self) -> usize {
        // 总额度不能反向截断用户设置的单进程额度。
        self.total_memory_mb.max(self.process_memory_mb) * 1024 * 1024
    }

    pub(crate) fn workset_bytes(self) -> u64 {
        self.workset_mb as u64 * 1024 * 1024
    }

    fn validate(self) -> std::io::Result<()> {
        for (key, value) in [
            ("thumb_fast_threads", self.fast_threads),
            ("thumb_tail_threads", self.tail_threads),
            ("thumb_gpu_inflight", self.gpu_inflight),
            ("thumb_gpu_per_adapter", self.gpu_per_adapter),
            ("thumb_process_memory_mb", self.process_memory_mb),
            ("thumb_total_memory_mb", self.total_memory_mb),
            ("thumb_workset_mb", self.workset_mb),
        ] {
            let def = crate::config::schema::SettingDef::find(key).expect("thumbnail setting");
            let crate::config::schema::SettingKind::Int { min, max } = def.kind else {
                unreachable!()
            };
            if value < min as usize || value > max as usize {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, key));
            }
        }
        Ok(())
    }
}

impl Default for ThumbnailLimits {
    fn default() -> Self {
        Self::from_reader(|_| None)
    }
}

static LIMITS: OnceLock<ThumbnailLimits> = OnceLock::new();

/// 在创建调度器、Job 或 GPU 会话前安装启动快照。
pub fn install(limits: ThumbnailLimits) -> std::io::Result<()> {
    limits.validate()?;
    LIMITS.set(limits).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "thumbnail limits initialized",
        )
    })
}

/// 获取当前进程启动时固定的资源设置。
pub fn get() -> &'static ThumbnailLimits {
    LIMITS.get_or_init(ThumbnailLimits::default)
}
