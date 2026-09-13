// src-tauri/src/ai/remote_registry.rs
//! 动态发现自托管仓库 `gficcg/clip_cn_vit-onnx` 中可下载的 ONNX，按「架构文件夹 → batch 变体」分类。
//! Dynamically discover downloadable ONNX in the self-hosted `gficcg/clip_cn_vit-onnx` repo,
//! classified by "architecture folder → batch variant".
//!
//! # 为什么动态发现
//! 仓库按架构分文件夹（clip_cn_vit-b-16 / -h-14 / -l-14-336 / -l-14），每个文件夹下提供图像塔的
//! 多个固定 batch 导出（bN）+ 一个动态 batch（dyn）+ 一个文本塔，且未来还会增删。把这些写死在
//! profile.rs 既冗长又易过期 —— 改为运行时拉 HF tree API 列文件，按命名约定归类。
//!
//! # 关键点
//! - 所有 onnx 都是 LFS 文件，tree API 的 `lfs.oid` **即文件 sha256**、`lfs.size` 即真实大小，
//!   故可直接生成「带校验」的下载清单（大小 + sha256），无需另抓清单。
//! - 仅图像塔有 batch 变体（文件名含 `.img.<bN|dyn>.`）；文本塔单一（`.txt.`）。
//! - `.onnx` 头与同名 `.extra_file`（外部权重）必须成对下载、同目录共存。
//! - 结果带 10 分钟模块级缓存，供 `list_model_registry` 与 `download_model` 共用，避免重复联网。

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::ai::profile::ModelAsset;

/// 自托管仓库 id（全部 ViT 系列 fp32 ONNX）。
pub const REPO: &str = "gficcg/clip_cn_vit-onnx";

/// 发现结果缓存有效期(内存 L1 与磁盘 L2 的「免联网」窗口共用)。
const CACHE_TTL: Duration = Duration::from_secs(600);

/// 磁盘 L2 缓存文件名(落 models 目录,与它描述的模型共存;Part4-T8/A5 持久化)。
pub const DISK_CACHE_FILE: &str = "registry_discovery.cache.json";

/// 模块级缓存：内容与镜像偏好无关（同一份文件树），命中即返回。
static CACHE: Mutex<Option<(Instant, Vec<DiscoveredArch>)>> = Mutex::new(None);

/// 图像塔 batch 轴类型：固定大小 `k` 或动态（任意批）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BatchKind {
    Dynamic,
    Fixed(u32),
}

/// 一个可下载文件（onnx 头或其 extra_file）的远程信息。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteFile {
    /// 仓库内相对路径，如 `clip_cn_vit-l-14/vit-l-14.img.b8.fp32.onnx`。
    pub path: String,
    /// 落地文件名（basename，扁平存入 models 目录）。
    pub file: String,
    pub size_bytes: u64,
    /// LFS 文件的 sha256（来自 `lfs.oid`）；非 LFS 文件为 None。
    pub sha256: Option<String>,
}

/// 一个图像塔 batch 变体（含其 `.extra_file`）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImageVariant {
    pub batch: BatchKind,
    pub onnx: RemoteFile,
    pub extra: Option<RemoteFile>,
}

/// 一个架构（= 仓库的一个文件夹）发现到的图像变体 + 共享文本塔。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiscoveredArch {
    pub folder: String,
    pub variants: Vec<ImageVariant>,
    pub text_onnx: Option<RemoteFile>,
    pub text_extra: Option<RemoteFile>,
}

// ── HF tree API 反序列化（仅取所需字段）────────────────────────────────────────
// ── HF tree API deserialisation (only the fields we need) ──────────────────────

#[derive(Deserialize)]
struct TreeEntry {
    #[serde(rename = "type")]
    kind: String,
    path: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    lfs: Option<Lfs>,
}

#[derive(Deserialize)]
struct Lfs {
    /// LFS object id = 文件 sha256（小写 hex）。
    oid: String,
    #[serde(default)]
    size: u64,
}

/// 从图像塔 onnx 文件名解析 batch 类型。
/// 例：`vit-l-14.img.b8.fp32.onnx` → Fixed(8)；`...img.dyn...` → Dynamic；
/// `vit-b-16.img.fp16.onnx`（eisneim 静态，batch 钉死为 1）→ None（无 `bN`/`dyn` 标记）。
pub fn parse_batch(file: &str) -> Option<BatchKind> {
    let after = file.split(".img.").nth(1)?; // "b8.fp32.onnx" / "dyn.fp32.onnx" / "fp16.onnx"
    let tok = after.split('.').next()?; // "b8" / "dyn" / "fp16"
    if tok == "dyn" {
        return Some(BatchKind::Dynamic);
    }
    let n: u32 = tok.strip_prefix('b')?.parse().ok()?;
    Some(BatchKind::Fixed(n))
}

/// 由远程文件构造一个带主源 + hf-mirror 镜像 + 大小/sha256 校验的下载资产。
pub fn remote_asset(rf: &RemoteFile) -> ModelAsset {
    ModelAsset {
        url: format!("https://huggingface.co/{REPO}/resolve/main/{}", rf.path),
        mirror_url: Some(format!(
            "https://hf-mirror.com/{REPO}/resolve/main/{}",
            rf.path
        )),
        dest: rf.file.clone(),
        size_bytes: rf.size_bytes,
        sha256: rf.sha256.clone(),
    }
}

/// 拉取并解析仓库文件树，按文件夹归类为各架构的图像变体 + 文本塔。
/// `mirror_first` 仅影响首选连接的主机（命中缓存时忽略）。
///
/// 三级来源(Part4-T8/A5 持久化,2026-07-02):
/// 1. 内存 L1(10min,进程内):命中即返回;
/// 2. 磁盘 L2 新鲜(< TTL):冷启动免重复联网(重启后 10min 内直接用上次快照);
/// 3. 联网拉取:成功 → 更新 L1 + 原子写 L2;**失败 → 任意年龄的 L2 兜底**(离线场景关键:
///    陈旧清单仍可浏览/生成下载清单,失败原因保留在 warn),彻底无缓存才返回 Err。
///
/// `disk_cache=None` 时行为与旧版一致(纯内存,测试/无目录场景)。
pub async fn discover(
    mirror_first: bool,
    disk_cache: Option<&Path>,
) -> Result<Vec<DiscoveredArch>, String> {
    // 命中内存缓存（克隆后立即释放锁，不跨 await 持锁）。
    {
        let guard = CACHE.lock().unwrap();
        if let Some((t, v)) = guard.as_ref() {
            if t.elapsed() < CACHE_TTL {
                return Ok(v.clone());
            }
        }
    }

    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // 磁盘 L2 新鲜 → 免联网(文件仅 KB 级,同步读的代价可忽略)。
    if let Some(p) = disk_cache {
        if let Some((age, archs)) = load_disk_cache(p, now_unix) {
            if age < CACHE_TTL.as_secs() {
                let mut guard = CACHE.lock().unwrap();
                *guard = Some((Instant::now(), archs.clone()));
                return Ok(archs);
            }
        }
    }

    match fetch_tree(mirror_first).await {
        Ok(entries) => {
            let archs = classify(entries);
            {
                let mut guard = CACHE.lock().unwrap();
                *guard = Some((Instant::now(), archs.clone()));
            }
            if let Some(p) = disk_cache {
                store_disk_cache(p, &archs, now_unix);
            }
            Ok(archs)
        }
        Err(e) => {
            // 联网失败 → 任意年龄磁盘兜底;同时入 L1,10min 内不再反复打网络。
            if let Some(p) = disk_cache {
                if let Some((age, archs)) = load_disk_cache(p, now_unix) {
                    tracing::warn!(
                        "动态发现联网失败,用磁盘缓存兜底(age={age}s) | discovery offline fallback: {e}"
                    );
                    let mut guard = CACHE.lock().unwrap();
                    *guard = Some((Instant::now(), archs.clone()));
                    return Ok(archs);
                }
            }
            Err(e)
        }
    }
}

/// 磁盘 L2 缓存条目。`fetched_at` = 拉取时刻(unix 秒),读取方据此算 age。
#[derive(Serialize, Deserialize)]
struct DiskCache {
    fetched_at: u64,
    archs: Vec<DiscoveredArch>,
}

/// 读磁盘缓存 → `(age_secs, archs)`;缺失/损坏/无法解析一律 None(按无缓存处理,不报错)。
fn load_disk_cache(path: &Path, now_unix: u64) -> Option<(u64, Vec<DiscoveredArch>)> {
    let raw = std::fs::read(path).ok()?;
    let dc: DiskCache = serde_json::from_slice(&raw).ok()?;
    Some((now_unix.saturating_sub(dc.fetched_at), dc.archs))
}

/// 原子写盘(先 `*.json.tmp` 再同卷 rename,派生产物落盘纪律):半截文件绝不会被
/// `load_disk_cache` 读到。best-effort:失败仅 warn,不影响在线主流程。
fn store_disk_cache(path: &Path, archs: &[DiscoveredArch], now_unix: u64) {
    let dc = DiskCache {
        fetched_at: now_unix,
        archs: archs.to_vec(),
    };
    let bytes = match serde_json::to_vec(&dc) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("动态发现缓存序列化失败 | registry cache serialize failed: {e}");
            return;
        }
    };
    let tmp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&tmp, &bytes).and_then(|()| std::fs::rename(&tmp, path)) {
        tracing::warn!("动态发现缓存写盘失败 | registry cache persist failed: {e}");
    }
}

/// 拉取 tree JSON：按偏好顺序尝试官方源与 hf-mirror，任一成功即返回。
async fn fetch_tree(mirror_first: bool) -> Result<Vec<TreeEntry>, String> {
    let hosts: [&str; 2] = if mirror_first {
        ["https://hf-mirror.com", "https://huggingface.co"]
    } else {
        ["https://huggingface.co", "https://hf-mirror.com"]
    };
    let path = format!("/api/models/{REPO}/tree/main?recursive=true");
    // 安全 client(2026-07-10 审查 B9):原裸 Client::new() 无 connect/整体超时,恶劣网络下
    // discover() 可挂到 OS 级 TCP 超时;SmallFile=连接 15s+整体 300s,并继承拒绝重定向降级。
    let client = crate::download::secure_client(crate::download::TimeoutPolicy::SmallFile)
        .map_err(|e| format!("HTTP 客户端构建失败: {e}"))?;

    let mut last_err = String::from("no host tried");
    for host in hosts {
        let url = format!("{host}{path}");
        match client
            .get(&url)
            .header("User-Agent", "scrollery")
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => match resp.json::<Vec<TreeEntry>>().await {
                Ok(v) => return Ok(v),
                Err(e) => last_err = format!("解析失败 {url}: {e}"),
            },
            Ok(resp) => last_err = format!("HTTP {} @ {url}", resp.status().as_u16()),
            Err(e) => last_err = format!("{url}: {e}"),
        }
    }
    Err(last_err)
}

/// 把扁平文件列表归类为各架构的图像变体 + 共享文本塔。无图像 onnx 的架构（如尚未导出的 h-14）跳过。
fn classify(entries: Vec<TreeEntry>) -> Vec<DiscoveredArch> {
    use std::collections::BTreeMap;

    // folder → [(basename, RemoteFile)]
    let mut by_folder: BTreeMap<String, Vec<(String, RemoteFile)>> = BTreeMap::new();
    for e in entries {
        if e.kind != "file" {
            continue;
        }
        let Some((folder, base)) = e.path.split_once('/') else {
            continue;
        };
        if !folder.starts_with("clip_cn_vit") {
            continue;
        }
        // 仅关心 onnx 头与其 extra_file。
        if !(base.ends_with(".onnx") || base.ends_with(".onnx.extra_file")) {
            continue;
        }
        let (size, sha256) = match &e.lfs {
            Some(l) => (l.size, Some(l.oid.clone())),
            None => (e.size, None),
        };
        let rf = RemoteFile {
            path: e.path.clone(),
            file: base.to_string(),
            size_bytes: size,
            sha256,
        };
        by_folder
            .entry(folder.to_string())
            .or_default()
            .push((base.to_string(), rf));
    }

    let mut out = Vec::new();
    for (folder, files) in by_folder {
        // basename → RemoteFile（用于把 onnx 头关联到同名 .extra_file）。
        let index: std::collections::HashMap<&str, &RemoteFile> =
            files.iter().map(|(b, f)| (b.as_str(), f)).collect();

        let mut variants: Vec<ImageVariant> = Vec::new();
        let mut text_onnx = None;
        let mut text_extra = None;

        for (base, rf) in &files {
            if base.ends_with(".extra_file") {
                continue; // extra 由对应 onnx 头关联，不单独成项
            }
            let extra_name = format!("{base}.extra_file");
            let extra = index.get(extra_name.as_str()).map(|f| (*f).clone());

            if base.contains(".img.") {
                if let Some(batch) = parse_batch(base) {
                    variants.push(ImageVariant {
                        batch,
                        onnx: rf.clone(),
                        extra,
                    });
                }
            } else if base.contains(".txt.") {
                text_onnx = Some(rf.clone());
                text_extra = extra;
            }
        }

        if variants.is_empty() {
            continue;
        }
        // 固定 batch 升序在前，dyn 垫底，UI 展示更直观。
        variants.sort_by_key(|v| match v.batch {
            BatchKind::Fixed(k) => k as i64,
            BatchKind::Dynamic => i64::MAX,
        });
        out.push(DiscoveredArch {
            folder,
            variants,
            text_onnx,
            text_extra,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arch(folder: &str) -> DiscoveredArch {
        DiscoveredArch {
            folder: folder.into(),
            variants: Vec::new(),
            text_onnx: None,
            text_extra: None,
        }
    }

    /// 磁盘 L2:原子写 → 读 roundtrip,age 按 fetched_at 计;损坏文件安全返回 None。
    #[test]
    fn disk_cache_roundtrip_and_corruption() {
        let dir = std::env::temp_dir().join(format!("reg-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("cache.json");

        store_disk_cache(&p, &[arch("clip_cn_vit-b-16")], 1_000);
        let (age, archs) = load_disk_cache(&p, 1_600).expect("应可读回");
        assert_eq!(age, 600);
        assert_eq!(archs.len(), 1);
        assert_eq!(archs[0].folder, "clip_cn_vit-b-16");
        assert!(
            !p.with_extension("json.tmp").exists(),
            "原子写不得残留 tmp 文件"
        );

        std::fs::write(&p, b"{corrupt").unwrap();
        assert!(load_disk_cache(&p, 2_000).is_none(), "损坏文件按无缓存处理");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 图像塔文件名 batch 解析契约(b8/dyn/无标记/文本塔)。
    #[test]
    fn parse_batch_naming() {
        assert_eq!(
            parse_batch("vit-l-14.img.b8.fp32.onnx"),
            Some(BatchKind::Fixed(8))
        );
        assert_eq!(
            parse_batch("vit-l-14.img.dyn.fp32.onnx"),
            Some(BatchKind::Dynamic)
        );
        assert_eq!(parse_batch("vit-b-16.img.fp16.onnx"), None);
        assert_eq!(parse_batch("vit-b-16.txt.fp32.onnx"), None);
    }
}
