# 冷门格式插件子系统 · Part 1 地基 | Foundation（P0–P2）

> 🔴 **已废弃(2026-07-10 文档治理补标)**:exotic v2 分卷,被 v3.1 同名分卷取代(见同目录 exotic_format_plugin_plan/)。

> 本卷范围：**schema 迁移 + 模块骨架 + 扫描识别 + 主路径让路 + 宿主路由 + 门控查询**。
> 落完即可：冷门项被识别入库并标记、`format→插件`路由可查、门控状态可查询——
> **此时前端即可渲染占位（不依赖后续 Part）**。本卷**不含 worker、不含 pipeline**。
>
> 配套总纲：`exotic_format_plugin_plan_v2.md`（务必先读其 §2 现状、§4 架构、§5 决策、§6 数据模型）。

---

## 本卷前置依赖 | Prerequisites

- 无（本卷是整个子系统的第一步）。
- **施工第一动作**：`grep -n "if version <" src-tauri/src/db/migration.rs` 确认当前最高 schema 版本，本卷 DDL 用"当前最高 +1"作为 `V_next`（D9，避免与人脸 TODO2 撞号）。

## 本卷完成定义 | DoD

1. `cargo build` + `clippy` 零新警告；既有迁移幂等（重启不重复执行）。
2. 一张 `.psd` 与一个 `.rmvb`（B 类）扫描后**都入库**，`media_type` 正确，`exotic_status=0`。
3. 主缩略图路径对 `.psd` **不再报 `UnsupportedFormat`**，而是"让路"（`thumb_status` 维持 0，无 error 日志）。
4. 后端命令 `exotic_format_status()` 返回"格式→{installed,authorized,available}"映射；对放入一份**测试 manifest**（无需 worker）的格式，能正确路由并标注门控态。
5. 全程**无任何 worker/pipeline**，纯地基可独立验收。

---

## Phase 0 — 骨架 + schema + 配置 | Skeleton, schema, config

### 0.1 目标
建 `exotic` 模块骨架、`V_next` 迁移、config 默认值。为后续所有 Phase 提供编译可过的空壳。

### 0.2 改动文件
- 新增 `src-tauri/src/exotic/mod.rs`（模块根，pub 子模块声明）
- 新增 `src-tauri/src/exotic/host.rs`（P2 填实，先放空壳 + 类型）
- 修改 `src-tauri/src/lib.rs`（`mod exotic;` + 注册命令，命令本体在 P2）
- 修改 `src-tauri/src/db/schema.rs`（追加 `SCHEMA_V_next` 常量）
- 修改 `src-tauri/src/db/migration.rs`（追加 `if version < V_next` 块）

### 0.3 schema（完整带注释 DDL）

```rust
// src-tauri/src/db/schema.rs 追加（V_next = 当前最高 +1，落地时确认）
// 冷门格式子系统 schema | Exotic-format subsystem schema.
pub const SCHEMA_V_next: &str = "
-- ── media_items.exotic_status：冷门格式处理状态机（仿 ai_status / face_status）──────
-- 0=待处理 pending / 1=处理中 processing / 2=完成 done / 3=错误 error
-- 注：门控态（需购买/无插件）不入此列（见总纲 O1 倾向 a），由 host 路由 + exotic_format_status 命令实时判定。
ALTER TABLE media_items ADD COLUMN exotic_status INTEGER NOT NULL DEFAULT 0;
-- 仅索引未完成/出错（0/1/3），生产者扫描命中极小。
CREATE INDEX IF NOT EXISTS idx_media_exotic ON media_items(exotic_status)
                                            WHERE exotic_status IN (0,1,3);

-- ── exotic_plugins：已安装插件缓存表（磁盘 manifest 的可查询投影）──────────────────
-- 启动时由 host 扫描 plugins/ 重建；authorized 由 license 校验后写入（Part3）。
CREATE TABLE IF NOT EXISTS exotic_plugins (
    id            TEXT PRIMARY KEY,           -- manifest.id（= license/路由/目录键）
    name          TEXT NOT NULL,
    version       TEXT NOT NULL,
    media_kind    TEXT NOT NULL,              -- image|video|audio|document
    formats       TEXT NOT NULL,              -- JSON 数组 [\"psd\",\"psb\"]
    capabilities  TEXT NOT NULL,              -- JSON 数组 [\"thumbnail\",\"metadata\"]
    license_tier  TEXT NOT NULL,              -- free|paid
    authorized    INTEGER NOT NULL DEFAULT 0, -- license 校验通过=1（启动刷新）
    commercial_ok INTEGER NOT NULL DEFAULT 0,
    installed_at  INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
";
```

```rust
// src-tauri/src/db/migration.rs 在最后一个 if 块后追加（替换占位注释 // if version < 9）
// 冷门格式子系统 | Exotic-format subsystem。
if version < V_NEXT {
    conn.execute_batch(schema::SCHEMA_V_next)?;
    // config 默认值（仿 face_* 开关）。INSERT OR IGNORE 幂等。
    conn.execute_batch(
        "INSERT OR IGNORE INTO app_config (key, value) VALUES
            ('exotic_enabled',      '1'),   -- 总开关 master switch
            ('exotic_auto_process', '1'),   -- 随扫描自动处理已授权格式
            ('exotic_max_workers',  '0'),   -- 0=自动（按 manifest.limits 与核数）
            ('exotic_dev_mode',     '0');", -- 1=跳过 license 门控 + 允许手放 worker（开发期）
    )?;
}
```

> ⚠️ `V_NEXT` 用实际数字替换（如当前最高是 8 则为 9；若人脸 TODO2 先落则为 10）。**两处常量名也相应改**（`SCHEMA_V9`/`if version < 9`）。

### 0.4 模块骨架

```rust
// src-tauri/src/exotic/mod.rs
//! 冷门格式插件子系统 | Exotic-format plugin subsystem.
//! 独立于主引擎的 worker 插件处理线（见 plan-docs/exotic_format_plugin_plan_v2.md）。
pub mod host;       // P2：插件发现 + 路由表 + 门控
pub mod manifest;   // P2：manifest.json 解析与校验
// pub mod protocol; // Part2 P3：worker IPC 协议（或独立 crate，见 O5）
// pub mod pipeline; // Part2 P4：独立流水线
// pub mod license;  // Part3 P5：Ed25519 验签
// pub mod registry; // Part3 P6：远程注册表
// pub mod package;  // Part3 P6：插件包解包/校验
```

### 0.5 验收
`cargo build` 通过；删库重启触发迁移、二次重启不重复执行；`app_config` 含 4 个 exotic_* 键；`exotic_plugins` 表存在。

## Phase 1 — 扫描识别 + 主路径让路 | Scan recognition & main-path yield

### 1.1 目标
两件事：① 让 **B 类未识别格式**（rmvb 等）能入库；② 让主缩略图路径对**冷门格式让路**（不报错、不占主池）。

### 1.2 B 类识别：扩展分类（务必谨慎）

现状（已核验）：`utils/format.rs:36 classify_media_type` 对 rmvb/rm 返回 `None` → walker 跳过。两种改法：

**方案 A（推荐）**：在 `classify_media_type` 直接补全冷门扩展名，使其返回对应 `MediaType`。

```rust
// utils/format.rs classify_media_type 内补充（小写、无点）
// ── 冷门视频 exotic video（B 类，补识别）──────────────────────────
"rmvb" | "rm" | "rmhd" => Some(MediaType::Video),
// ── 冷门图像 exotic image（A 类已有 psd/heic/raw；如需补 ai/eps 等）──
// "ai" | "eps" => Some(MediaType::Image),  // 视需要
```

> ⚠️ **取舍**：分类表认领某扩展名 = 主路径会尝试处理它。故补 B 类后**必须同时做 §1.3 让路**，否则 rmvb 会走主视频路径再次失败。A 类（psd 等）本已在表中，§1.3 让路对其同样适用。

**方案 B**：单独维护 `exotic_extra_formats()` 表，walker 对其入库但标记来源。**不推荐**——增加第二套识别真相，违背"单一识别层"。

### 1.3 主路径让路：generator 分派前判断

现状（已核验）：`thumbnail/generator.rs:163 decode_media_step_inner` 按 `media_type` 分派；image 类走 arena，`arena.engine_for("psd")=None` → `:290 UnsupportedFormat`；非 image 类 → `:271 thumb_status=2`。

改动：在 `decode_media_step_inner` **最前**（`thumb_status==1` 命中缓存判断之后、media_type 分派之前）插入让路分支：

```rust
// generator.rs decode_media_step_inner 内，分派前
// 冷门格式让路：不在主路径解码，交还冷门 pipeline（保持 G1）。
// 用 host 内存路由表判断，避免每项查 DB。host 句柄经 AppState 注入（见 P2）。
if state.exotic_host.claims_format(&item.file_format) {
    return Ok(DecodeResult::Ready(ThumbResult {
        item_id,
        thumb_status: 0,        // 维持未生成；真正处理由 exotic_status 驱动
        thumb_path: None,
        thumbhash: None,
    }));
}
```

> `claims_format` 是 host 的 O(1) 内存查询（`HashSet<String>` 或路由表 keys）。**不查 DB、不阻塞**。host 句柄如何到达 generator：见 P2.4。

### 1.4 扫描入库时初始化 exotic_status

walker/enricher 入库冷门项时 `exotic_status` 默认 0（DDL 已 `DEFAULT 0`，通常无需显式写）。**确认** `media_items` 插入路径不会漏列即可。

> 注意 P0 基础 bug（见 [[feature-expansion-plan-v1]]）：非 image 项的 0×0 尺寸问题若仍存在，冷门视频/文档需给 per-type 默认尺寸（video 16:9 等）以免 justified 布局崩。本卷不强制修，但**若 B 类视频入库后布局异常，回看该基础 bug**。

### 1.5 验收
- 放一个 `.rmvb` 进扫描根 → 入库，`media_type=video`，`exotic_status=0`。
- 放一个 `.psd` → 入库，`media_type=image`；扫描日志**无 `UnsupportedFormat`**；`thumb_status` 保持 0（让路生效）。
- 主路径对常见格式（jpg/mp4）行为**完全不变**（让路分支只命中冷门）。

## Phase 2 — 宿主路由 + 门控查询 | Plugin host: routing & gating

### 2.1 目标
`exotic/host.rs` 启动扫描 `plugins/*/manifest.json` → 建内存 `format→plugin` 路由表 → 查 license 标注门控态 → 提供 `claims_format()`（给 generator）与 `exotic_format_status()`（给前端）。**本卷 license 校验先用桩**（`exotic_dev_mode=1` 视为已授权，或读 `exotic_plugins.authorized`），真实 Ed25519 验签在 Part3 P5 替换。

### 2.2 manifest 解析 | `exotic/manifest.rs`

清单是主程序↔插件唯一契约。**主程序对插件的全部认知都来自它**（G3 根基）。

```rust
// src-tauri/src/exotic/manifest.rs
//! 插件清单解析 | Plugin manifest parsing. 主程序↔插件唯一契约。
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub schema: u32,               // manifest 协议版本（向后兼容靠它）
    pub id: String,                // 全局唯一插件 id（= license/路由/目录键）
    pub name: String,
    pub version: String,
    pub vendor: String,
    pub protocol_version: u32,     // worker IPC 协议版本（Part2 P3 握手校验）
    pub media_kind: MediaKind,     // image|video|audio|document（决定产物落地）
    pub formats: Vec<String>,      // 认领的扩展名（小写无点）
    pub capabilities: Vec<String>, // worker 支持的 op：thumbnail/metadata/text
    pub license: LicenseSpec,
    pub commercial_ok: bool,       // 内含解码库是否允许商业分发（§7 亲验）
    pub min_host_version: String,
    pub bin: std::collections::HashMap<String, String>, // target triple → 相对可执行路径
    pub limits: Limits,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind { Image, Video, Audio, Document }

#[derive(Debug, Clone, Deserialize)]
pub struct LicenseSpec { pub tier: String, pub sku: String } // tier: free|paid

#[derive(Debug, Clone, Deserialize)]
pub struct Limits {
    pub max_concurrency: u32,   // 同时最多几个 worker 进程
    pub task_timeout_ms: u64,   // 单任务超时 → 杀进程标错误
    pub mem_soft_limit_mb: u64,
}

impl Manifest {
    /// 从目录加载并基本校验（字段齐全、schema 受支持、bin 非空）。
    pub fn load(dir: &std::path::Path) -> Result<Self, crate::error::AppError> { todo!() }
    /// 当前平台的 worker 相对路径（按 target triple 查 bin）。缺则该平台不支持。
    pub fn bin_for_current_platform(&self) -> Option<&str> { todo!() }
}
```

示例 `manifest.json`（PSD，Part2 P3 配套 worker）：

```jsonc
{
  "schema": 1,
  "id": "exotic-image-psd",
  "name": "PSD 图像引擎 | PSD Image Engine",
  "version": "1.0.0",
  "vendor": "Picasa Next",
  "protocol_version": 1,
  "media_kind": "image",
  "formats": ["psd", "psb"],
  "capabilities": ["thumbnail", "metadata"],
  "license": { "tier": "paid", "sku": "psd-engine-2026" },
  "commercial_ok": true,
  "min_host_version": "0.1.0",
  "bin": {
    "x86_64-pc-windows-msvc": "bin/psd-worker-x86_64-pc-windows-msvc.exe",
    "aarch64-apple-darwin":   "bin/psd-worker-aarch64-apple-darwin"
  },
  "limits": { "max_concurrency": 2, "task_timeout_ms": 30000, "mem_soft_limit_mb": 1024 }
}
```

`✶ 扩展插口的本质 ────────────────────────`
主程序代码里**没有一处** `if format == "psd"`。它只认 manifest 字段：`formats` 决定路由、
`capabilities` 决定能发哪些 op、`media_kind` 决定产物往哪落、`bin` 决定启哪个可执行。
**加一个 RMVB 插件 = 上架一个新 manifest + worker，主程序一行不改**——这就是 G3。
`──────────────────────────────────────────`

### 2.3 host 路由 + 门控 | `exotic/host.rs`

```rust
// src-tauri/src/exotic/host.rs
//! 插件宿主 | Plugin host：发现插件、建路由表、门控。
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

/// 单格式门控态（供前端占位渲染）。 | Per-format gating state.
#[derive(serde::Serialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FormatGate {
    Authorized,  // 已装 + 已授权 → 进处理
    NeedsPurchase, // 已装/可购但未授权 → 占位引导购买
    NoPlugin,    // 无插件认领（理论上不会进 host，但前端可能查到）
}

pub struct ExoticHost {
    /// format（小写） → plugin_id。内存路由表，启动构建，随安装/卸载刷新。
    routes: RwLock<HashMap<String, String>>,
    /// 已加载 manifests（plugin_id → Manifest）。
    plugins: RwLock<HashMap<String, super::manifest::Manifest>>,
    /// 已授权 plugin_id 集合（本卷由 dev_mode 或 exotic_plugins.authorized 决定；Part3 换 license）。
    authorized: RwLock<HashSet<String>>,
    /// claims_format 的快速集合（routes 的 keys 投影，O(1) 命中）。
    claimed: RwLock<HashSet<String>>,
}

impl ExoticHost {
    /// 启动/刷新：扫描 {app_data}/plugins/*/manifest.json，重建路由表与授权集。
    /// 冲突（两插件认领同 format）→ 取已授权者，否则版本高者，并告警。
    pub fn refresh(&self, plugins_dir: &std::path::Path, db: &crate::db::Db) { todo!() }

    /// generator 让路用：该 format 是否被任一插件认领（O(1)，不查 DB）。
    pub fn claims_format(&self, fmt: &str) -> bool {
        self.claimed.read().unwrap().contains(&fmt.to_ascii_lowercase())
    }

    /// 某 format 的门控态（前端占位 + Producer 领取判定）。
    pub fn gate_of(&self, fmt: &str) -> FormatGate { todo!() }

    /// Producer 用：format 是否"已装+已授权"可处理。
    pub fn is_processable(&self, fmt: &str) -> bool {
        matches!(self.gate_of(fmt), FormatGate::Authorized)
    }
}
```

### 2.4 host 句柄注入 AppState

generator（P1.3）与 pipeline（Part2）都需 host。在 `AppState` 加 `pub exotic_host: Arc<ExoticHost>`，`lib.rs` 启动时 `refresh()` 一次（扫描 plugins_dir）。

```rust
// state.rs 追加
pub exotic_host: std::sync::Arc<crate::exotic::host::ExoticHost>,
```

> 启动顺序：DB 迁移 → 构建 AppState（含 exotic_host）→ `exotic_host.refresh(plugins_dir, &db)` → 启动扫描。`refresh` 同时**重建 `exotic_plugins` 缓存表**（磁盘 manifest 为真相）。

### 2.5 命令：`exotic_format_status` | `ipc/exotic_commands.rs`

```rust
// src-tauri/src/ipc/exotic_commands.rs（新）
//! 冷门子系统命令 | Exotic subsystem IPC commands.

/// 前端查"格式→门控态"，用于 MediaThumb 占位渲染与设置页插件市场。
#[derive(serde::Serialize)]
pub struct FormatStatus {
    pub format: String,
    pub plugin_id: Option<String>,
    pub media_kind: Option<String>,
    pub gate: super::super::exotic::host::FormatGate, // authorized|needs_purchase|no_plugin
    pub license_tier: Option<String>,
}

#[tauri::command]
pub async fn exotic_format_status(
    state: tauri::State<'_, std::sync::Arc<crate::state::AppState>>,
) -> Result<Vec<FormatStatus>, String> { todo!() }

/// 列已安装插件（设置页"已装"区）。
#[tauri::command]
pub async fn list_exotic_plugins(/* ... */) -> Result<Vec<PluginInfo>, String> { todo!() }
```

注册：`lib.rs` 的 `invoke_handler` 加这两条；`ipc/mod.rs` 加 `pub mod exotic_commands;`。

### 2.6 验收
- 手放一份测试 manifest（`{app_data}/plugins/exotic-image-psd/manifest.json`，无 bin 也可，仅测路由）→ 重启 → `claims_format("psd")==true`、`exotic_format_status()` 含 psd 项。
- `exotic_dev_mode=1` 时 psd 门控态=Authorized；`=0` 且 `authorized=0` 时=NeedsPurchase。
- 主路径对 psd 让路（P1.3 接 `state.exotic_host`）生效，扫描无 UnsupportedFormat。

---

## 新会话续作提示词 | Continuation prompt（Part 1）

```
任务：实施 plan-docs/exotic_format_plugin_part1_foundation.md（冷门格式子系统·地基 P0–P2）。
先读：总纲 exotic_format_plugin_plan_v2.md（§2/§4/§5/§6）+ 本卷全文。
前置依赖：无。第一动作 = grep "if version <" src-tauri/src/db/migration.rs 定 V_next。
必看源码：
  - src-tauri/src/db/{schema.rs,migration.rs}（迁移范式）
  - src-tauri/src/utils/format.rs:36（classify_media_type）
  - src-tauri/src/thumbnail/generator.rs:163（decode_media_step_inner 分派）
  - src-tauri/src/state.rs（AppState 字段注入范式，参考 face_analysis_token）
  - src-tauri/src/ipc/mod.rs + lib.rs（命令注册范式，参考 face_commands）
施工顺序：P0 骨架+schema → P1 识别+让路 → P2 host 路由+门控+命令。
DoD（本卷末）：psd/rmvb 入库、主路径对 psd 让路无报错、exotic_format_status 可查门控。
约定：thiserror / rusqlite 参数绑定 / 中英双语注释与日志 / 中文 commit / 改动及时 commit。
注意：本卷不做 worker/pipeline；license 用桩（dev_mode 或 authorized 列），真实验签留 Part3。
完成后：回到 Part2（exotic_format_plugin_part2_pipeline_worker.md）。
```

## 本卷产出清单 | Deliverables checklist

- [ ] `db/schema.rs` `SCHEMA_V_next` + `db/migration.rs` `if version < V_next` 块（含 config 默认值）
- [ ] `exotic/mod.rs` + `exotic/manifest.rs`（Manifest 解析 + 平台 bin 选择）
- [ ] `exotic/host.rs`（refresh/claims_format/gate_of/is_processable + exotic_plugins 缓存表重建）
- [ ] `utils/format.rs` 补 B 类扩展名（rmvb 等）
- [ ] `thumbnail/generator.rs` 让路分支（接 `state.exotic_host`）
- [ ] `state.rs` 注入 `exotic_host: Arc<ExoticHost>`
- [ ] `ipc/exotic_commands.rs`（`exotic_format_status` + `list_exotic_plugins`）+ 注册
- [ ] 验收：psd/rmvb 入库、让路生效、门控可查

