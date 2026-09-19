# 冷门格式插件子系统 · Part 4 收口 | Frontend, Slice & Hardening（P7–P9）

> 🔴 **已废弃(2026-07-10 文档治理补标)**:exotic v2 分卷,被 v3.1 同名分卷取代(见同目录 exotic_format_plugin_plan/)。

> 本卷范围：**前端 exoticStore + 占位渲染 + 设置页插件市场 + PSD 端到端切片联调 + 上线前硬化/测试**。
> 落完即可：正式可发布的 PSD 垂直切片（识别→占位→购买引导→下载→激活→处理→画廊缩略图）。
>
> 配套：总纲 v2（§6 门控、§10 路线图）+ Part1/2/3（已完成）。

---

## 本卷前置依赖 | Prerequisites

- **Part1+2+3 完成**（后端全链路：识别/路由/pipeline/license/分发）。
- 核验前端现有范式：`stores/{aiStore,faceStore,configStore}.ts`、`views/SettingsView.vue`、`components/media/`（MediaThumb 所在）、`components/settings/`、i18n。
- 核验 `MediaThumb` 如何按 `thumb_status` 渲染（占位需在其分支加冷门态）。

## 本卷完成定义 | DoD

1. 冷门未授权项在画廊显示**锁占位**（格式名 + 锁角标），点击引导设置页/购买。
2. 设置页"插件市场"面板：列已装/可购插件，能触发下载（进度条）+ 激活（填 token）。
3. 免费用户点占位 → 引导购买页（外链）；付费已激活 → 直接处理。
4. PSD 端到端**正式路径**（非 dev_mode）走通：占位→购买引导→下载→激活→自动处理→缩略图。
5. 硬化项（P9）逐条过：超时/崩溃/内存/跨平台缺 bin/并发不降速/安全闸。
6. 单测 + 关键路径手测记录在案（仿人脸"编译级 + 关键单测，GUI 用户自测"惯例）。

---

## Phase 7 — 前端 UI | Frontend

### 7.1 目标
新增 `exoticStore.ts`；`MediaThumb` 加冷门占位分支；设置页加"插件市场"面板；接通购买/下载/激活流。

### 7.2 `stores/exoticStore.ts`（仿 aiStore/faceStore）

```ts
// src/stores/exoticStore.ts
// 冷门格式子系统前端状态 | Exotic-format subsystem store.
import { defineStore } from 'pinia'

interface FormatStatus { format: string; pluginId?: string; mediaKind?: string;
  gate: 'authorized' | 'needs_purchase' | 'no_plugin'; licenseTier?: string }
interface PluginInfo { id: string; name: string; version: string; mediaKind: string;
  formats: string[]; licenseTier: string; authorized: boolean; commercialOk: boolean }
interface RegistryEntry { id: string; name: string; formats: string[]; licenseTier: string;
  priceHint?: string; storeUrl?: string; sizeBytes: number; installed: boolean; authorized: boolean }

export const useExoticStore = defineStore('exotic', {
  state: () => ({
    formatGates: {} as Record<string, FormatStatus>, // format(小写) → 门控，占位渲染查它
    installed: [] as PluginInfo[],
    registry: [] as RegistryEntry[],
    downloadProgress: {} as Record<string, number>,
  }),
  getters: {
    // MediaThumb 用：该 format 是否需占位（命中冷门表且未授权）。
    gateOf: (s) => (fmt: string) => s.formatGates[fmt.toLowerCase()]?.gate,
  },
  actions: {
    async refreshGates() { /* invoke exotic_format_status → formatGates */ },
    async refreshInstalled() { /* invoke list_exotic_plugins */ },
    async refreshRegistry() { /* invoke list_exotic_registry */ },
    async install(id: string) { /* invoke install_exotic_plugin + Channel 进度 → downloadProgress */ },
    async activate(id: string, token: string) { /* invoke activate_exotic_plugin → refreshGates */ },
  },
})
```

启动时 `App.vue` 调 `refreshGates()`（与现有 aiStore/faceStore 初始化并列）。`db:media_enriched` 事件后画廊已自动刷新（后端 emit），无需前端额外拉取缩略图。

### 7.3 占位渲染 | Placeholder in `MediaThumb`

`MediaThumb` 现按 `thumb_status` 渲染。加冷门分支：**当 `exoticStore.gateOf(item.fileFormat)` 为 `needs_purchase`** → 渲染锁占位（灰底 + 格式名大写 + 锁角标 + "点击解锁"），点击 → 跳设置页插件市场并高亮对应插件（或直接开购买外链）。

```vue
<!-- MediaThumb 占位分支（伪代码）。区分两类： -->
<!-- needs_purchase：可购买 → 锁角标 + 引导； no_plugin：暂不支持 → 灰显格式名，不引导 -->
<div v-if="exoticGate === 'needs_purchase'" class="exotic-locked" @click="goPluginMarket(item.fileFormat)">
  <span class="fmt">{{ item.fileFormat.toUpperCase() }}</span>
  <LockIcon /> <span>{{ t('exotic.clickToUnlock') }}</span>
</div>
```

> 与"无插件可用"（`no_plugin`）区分：前者"可购买"（引导），后者"暂不支持"（仅灰显格式名，不引导）。若有 thumbhash 则用其作模糊底，否则灰底。

### 7.4 设置页插件市场 | `components/settings/ExoticPluginMarket.vue`

挂 `SettingsView`（仿 `FaceModelLibrary.vue` 卡片）。两区：
- **已安装**：列 `installed`，显示授权态；未授权显"激活"按钮（弹框填 token）。
- **可获取**：列 `registry` 中未装项，显价格/大小 + "购买"（外链 storeUrl）+ "下载安装"（已购后，进度条）。
- 顶部总开关：`exotic_enabled` / `exotic_auto_process`（接 configStore）。

### 7.5 购买/加载分流 | Free vs paid flow（用户需求 5）
- **免费用户**（无 token）点占位/购买 → 打开 `storeUrl` 外部购买页。
- **付费用户**（购后得 token）→ 插件市场"下载安装"→"激活"（填 token）→ 自动处理。
- dev_mode 下跳过门控（开发自测）。

### 7.6 i18n + 路由
新增 i18n key（`exotic.*`：clickToUnlock/market/install/activate/notSupported…，中英双语）；设置页加入口（若需独立路由）。

### 7.7 验收
- 未授权 .psd 在画廊显锁占位；点击进插件市场。
- 插件市场列出 psd（已装/可购），下载有进度，激活后占位消失、缩略图出现。

## Phase 8 — PSD 端到端切片联调 | PSD vertical slice end-to-end

### 8.1 目标
关闭 `exotic_dev_mode`，走**正式商业路径**全链路验证 PSD（G7）。这是整个 v1 切片的"合龙"。

### 8.2 端到端剧本 | E2E script

| 步 | 动作 | 期望 |
|---|---|---|
| 1 | 全新库扫描含 `.psd` 的目录 | psd 入库，画廊显**锁占位**（NeedsPurchase，因未激活） |
| 2 | 点占位 → 设置页插件市场 | 列出 `exotic-image-psd`（可购/已装未授权） |
| 3 | 点"购买" | 打开 storeUrl 外链（O2 起步=外部购买） |
| 4 | （模拟收到 token）插件市场"下载安装" | download_assets 拉包，进度条；sha256+验签通过；解包到 plugins/ |
| 5 | "激活"填 token | license::verify 通过 → keyring 存 → host 标 authorized |
| 6 | 自动处理（exotic_auto_process=1） | pipeline 领取 psd → worker 出 WebP → 回填 → **占位变缩略图** |
| 7 | 重启 App | keyring token 仍在 → 仍授权；已处理项缩略图直接命中缓存 |

### 8.3 厂商侧准备物 | Vendor-side artifacts（切片演示用）
- 一对 Ed25519 密钥（公钥入仓、私钥离线）。
- 签好的 `manifest.sig` + 一个测试 license token（subject=测试）。
- 打好的 `exotic-image-psd-1.0.0.zip`（含 win/mac worker + manifest + manifest.sig + icon + LICENSE-3RD-PARTY）。
- 一个静态 JSON 目录（本地 http 或 CDN）作 registry index + 包托管。

### 8.4 验收
全 7 步通过；常见格式（jpg/mp4）导入速度无可感知下降；DB `exotic_status=2`、`thumb_status=1`、缩略图落同档缓存目录。

---

## Phase 9 — 硬化 + 测试 | Hardening & testing

### 9.1 安全硬化 | Security
- [ ] 三道闸全接（sha256 传输 / manifest.sig 来源 / 启动前再校验，R2）。
- [ ] worker 以**低优先级**启动（Win `BELOW_NORMAL_PRIORITY_CLASS`；mac nice）。
- [ ] worker 工作目录限制；不授予多余 asset 协议目录（仅 `plugins/` 读图标资源）。
- [ ] license 私钥仅离线；仓库只含公钥；CI 不泄密钥。
- [ ] 下载源 https；包大小上限防 zip 炸弹（解包前查 size + 解包总量上限）。

### 9.2 稳定性硬化 | Stability
- [ ] 任务超时（`task_timeout_ms`）→ 杀进程 + 标 error + 补新 worker（R4）。
- [ ] worker 崩溃 → 自动重启 + 在处理项标 error；连续崩溃 N 次 → 禁用该插件 + 告警（防崩溃风暴）。
- [ ] 内存软上限：v1 记录 + 告警；v2 经 Job Object（Win）/ rlimit（*nix）强约束（R10）。
- [ ] 孤儿恢复：启动 `exotic_status=1→0`（R 续传）。
- [ ] 协议版本不匹配 → 拒启 + 提示升级（R7）。

### 9.3 性能调优旋钮 | Tuning knobs（独立、不影响主引擎，G2）

| 旋钮 | 来源 | 作用 |
|---|---|---|
| `exotic_max_workers` | app_config | 全局 worker 并发上限（0=自动） |
| `manifest.limits.max_concurrency` | 清单 | 单插件并发上限（重解码插件设低） |
| `manifest.limits.task_timeout_ms` | 清单 | 单任务超时 |
| `manifest.limits.mem_soft_limit_mb` | 清单 | worker 内存软上限 |
| 进程优先级 | host 启动时设 | 低于前台 |
| 让步 | `should_yield_exotic` | scan/thumb/derive/交互时暂停派发 |

### 9.4 测试 | Tests
- **单测**：协议帧编解码往返（JSON + 二进制）；license::verify 四类失败分支；manifest 解析/平台 bin 选择；host 路由冲突解决；Producer 领取判定（is_processable）。
- **worker 独立测**：psd-worker 喂样本 PSD（正常/畸形/超大/CMYK/16bit）→ 回 ThumbnailReady/Failed。
- **集成手测**（GUI，仿人脸惯例"编译级 + 关键单测 + 用户自测"）：§8.2 端到端剧本逐步记录。
- **回归**：常见格式（jpg/mp4/pdf）行为不变；让路分支只命中冷门。

### 9.5 跨平台 | Cross-platform（G5）
- [ ] manifest.bin 按 target triple；缺当前平台 → 该格式标"本平台暂不支持"（R9，不报错）。
- [ ] worker 路径/换行/可执行位（mac/Linux chmod +x 解包后处理）。
- [ ] Windows 进程创建隐藏控制台窗口（`CREATE_NO_WINDOW`）。

### 9.6 文档与回写
- [ ] 总纲 §5 待定项（O1–O5）落地结论回写。
- [ ] §7 许可矩阵"亲验结论"回写（尤其 PSD `psd` crate 许可、未来 HEIC/FFmpeg）。
- [ ] `architecture_notes.md` 增"冷门子系统"段（不变量、let路、令牌阶梯）。
- [ ] 更新记忆：新增 `exotic-format-subsystem` 记忆条目（关联 [[face-recognition-plan]]、[[feature-expansion-plan-v1]]、[[gotcha-background-yield-model]]）。

---

## 横向扩展：加第二个格式 | Adding the 2nd format（验证 G3 零改动）

PSD 切片合龙后，**加任意新格式应做到主程序零改动**，仅产出新插件包：

1. 写新 worker（按 §Part2 协议；选 §7 取材档）→ 编译各平台二进制。
2. 写新 manifest.json（formats/capabilities/media_kind/bin/bin_hashes/limits/commercial_ok）。
3. 签 manifest.sig + 签 license token。
4. 打包 zip + 上架 registry index。
5. 用户在插件市场购买/下载/激活——**主程序一行不改**。

若加新格式需要改主程序，说明契约有漏，回 §Part2 协议或 §Part1 host 修补契约（而非特判格式）。建议横向顺序：RAW(T1)→HEIC(T2 OS 委托)→RMVB(T3)→Office(T1 元数据)→Hi-Res 音频（见总纲附录 A）。

---

## 新会话续作提示词 | Continuation prompt（Part 4）

```
任务：实施 plan-docs/exotic_format_plugin_part4_frontend_slice_hardening.md（冷门子系统·收口 P7–P9）。
先读：总纲 v2（§6 门控、§10）+ Part1/2/3（确认已完成）+ 本卷全文。
前置依赖：Part1+2+3 完成（后端全链路）。
必看前端范式：
  - src/stores/{aiStore,faceStore}.ts（store 范式）
  - src/views/SettingsView.vue + src/components/settings/FaceModelLibrary.vue（设置卡片范式）
  - src/components/media/（MediaThumb，thumb_status 渲染分支）
  - App.vue（store 初始化并列）+ i18n
施工顺序：P7 exoticStore + 占位 + 插件市场 → P8 关 dev_mode 端到端剧本 → P9 安全/稳定/性能/测试/跨平台硬化 + 回写文档与记忆。
DoD（本卷末）：未授权占位 + 购买引导 + 下载激活 + 自动处理出缩略图；硬化逐条过；端到端剧本记录。
约定：Vue3 Composition+TS strict / 中英双语注释 / i18n 双语 / 中文 commit / 改动及时 commit。
完成后：PSD 切片发布就绪；按"横向扩展"节加下一个格式。
```

## 本卷产出清单 | Deliverables checklist

- [ ] `stores/exoticStore.ts`（gates/installed/registry + 5 actions）
- [ ] `MediaThumb` 冷门占位分支（needs_purchase 引导 / no_plugin 灰显）
- [ ] `components/settings/ExoticPluginMarket.vue`（已装/可购两区 + 下载进度 + 激活弹框）+ 挂 SettingsView
- [ ] 购买/加载分流（免费外链 / 付费下载激活）+ i18n 双语 key
- [ ] P8 端到端剧本 7 步通过（正式路径，非 dev_mode）
- [ ] P9 硬化清单逐条（安全 3 闸 / 超时崩溃内存 / 调优旋钮 / 跨平台缺 bin / 协议版本）
- [ ] 单测（协议帧 / license 四分支 / manifest / 路由冲突）+ worker 独立测 + 回归
- [ ] 回写：总纲 O1–O5 结论、§7 许可亲验、architecture_notes、记忆条目

---

## 全集完成 | Whole-set done

四卷 + 总纲全部实施完毕 = 冷门格式插件子系统 PSD 切片**可发布**，且具备：独立调度（G1/G2）、零改动扩展插口（G3）、付费门控（G4）、跨平台（G5）、轻量按需（G6）、合规先行（G8）。后续格式按"横向扩展"节增量上架。

