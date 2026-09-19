# 冷门格式插件子系统 v3.1 · Part 4 前端与发布（P7–P9）

> 📦 **已归档(2026-07-10 文档治理)**:exotic v3.1 文档集成员,随 v3 全套整目录归档(缘由见同目录总纲横幅);现行权威 = refactor_2026/Part6。

> 范围：前端状态、购买/安装/激活体验、正式 PSD E2E、安全/稳定/性能/跨平台发布门禁。
> 前置：Part1–Part3 全部完成。
> 版本：v3.1（已并入 `exotic_format_plugin_plan_v3.1_addendum.md` 的 R2/R6/R8）。
> 目标：不是“演示可跑”，而是 PSD 切片具备可发布证据。

## 本卷完成定义

1. 未安装、未授权、过期、不兼容、平台不支持、安装损坏、处理中、失败均有准确 UI。
2. 锁占位不会向主 thumbnail generator 发请求。
3. 激活后 Coordinator 自动处理；无需重启或手工启动。
4. 前端不把 `thumbnail` 能力误当成查看/编辑/播放能力。
5. 正式 Release、全新用户数据、签名包与真实 keyring 完成端到端。
6. 安全攻击集、故障注入、百万库 SQL、常见格式性能基准全部达标。
7. Windows/macOS 干净机器通过平台签名与启动验证。

## Phase 7 — 前端状态与操作流

### 7.1 Store

新增 `src/stores/exoticStore.ts`。可用性与处理状态分开：

```ts
export type ExoticAvailability =
  | 'availableUninstalled'
  | 'installedUnlicensed'
  | 'authorized'
  | 'licenseExpired'
  | 'unsupportedPlatform'
  | 'incompatibleHost'
  | 'invalidInstallation'
  | 'disabled'
  | 'noOffering'

export type ExoticTaskState =
  | 'none' | 'pending' | 'processing'
  | 'retryableError' | 'terminalError' | 'done'

export interface FormatResolution {
  format: string
  mediaKind: 'image' | 'video' | 'audio' | 'document'
  pluginId?: string
  capabilities: string[]
  availability: ExoticAvailability
  storeUrl?: string
  installedVersion?: string
}
```

Store 管理：format resolution 快照、installed/registry、每插件下载状态、全局 processing status。token 只存在激活弹框局部变量；调用完成立即清空，不进 Pinia 持久化、日志或错误遥测。

启动先读取后端内置/缓存 Catalog，再挂媒体网格。订阅：

```text
exotic:catalog-changed
exotic:installation-changed
exotic:license-changed
exotic:status-changed
```

事件只触发合并刷新；处理乱序与组件卸载，避免重复 listener。

### 7.2 MediaThumb

增加 computed：

```ts
const exotic = computed(() =>
  props.fileFormat ? exoticStore.resolutionOf(props.fileFormat) : undefined
)

const exoticBlocksCommonRequest = computed(() =>
  exotic.value?.capabilities.includes('thumbnail') && props.thumbStatus !== 1
)
```

`loadThumb()` 最前检查 `exoticBlocksCommonRequest`；命中即返回，不 emit `request-thumb`。模板优先级：

1. 已有有效缩略图；
2. exotic processing/retry；
3. 可购买/未授权锁占位；
4. 过期/平台不支持/不兼容/损坏；
5. 普通 text card/placeholder；
6. 普通实际图片。

锁 CTA 使用独立按钮并 `stopPropagation`，避免同时打开不支持的详情页。compact 模式只显示格式与小图标，不创建高成本文案 DOM。

### 7.3 能力感知

`thumbnail` 仅表示能生成封面。点击媒体后的能力独立判断：

- 无 `preview/playback/text` 能力：沿用现有“不支持查看”页面；
- 不因已有封面就尝试浏览器直接解码 PSD/RMVB；
- 后续插件增加预览能力时扩协议与路由，不在前端按格式名特判。

### 7.4 插件市场

新增 `components/settings/ExoticPluginMarket.vue`：

- 可获取：价格提示、平台、包大小、商店链接；
- 已购买未安装：下载安装；
- 已安装未授权：激活；
- 已授权：版本、更新、修复、卸载；
- 过期/不兼容/损坏：明确修复动作；
- 下载/验证/安装/回滚分别显示阶段，不能只显示百分比。

前端只把 plugin_id 交给后端。外链打开前校验后端返回 URL，并显示目标域名；禁止 Registry 文案注入 HTML。插件图标首发只用 PNG/WebP，不直接渲染不可信 SVG。

### 7.5 配置与可访问性

设置项：`exotic_enabled`、`exotic_auto_process`、`exotic_paused`、`exotic_max_workers`。关闭 enabled 停止调度但保留安装与产物；pause 与 disabled 文案分离。

所有状态具备中英 i18n、键盘焦点、ARIA label、非颜色提示。下载取消、激活失败、回滚成功等通知不得泄露本地路径、token、subject hash。

### 7.6 前端测试

- Store 全 Availability × TaskState 组合；
- MediaThumb 命中 exotic gate 时 `request-thumb` 发送次数为 0；
- 激活事件后占位→processing→done；
- token 调用结束即清空；
- 外链域名展示与非法 URL 拦截；
- compact、虚拟列表复用、`v-memo` 下状态仍刷新；
- 下载取消、组件卸载、事件乱序、重复 listener。

## Phase 8 — PSD 正式端到端

### 8.1 发布候选准备物

- Release Host；不带 `exotic-dev-fixtures`；
- 当前平台已签名/公证 PSD Worker；
- 签名 Registry index 与 package manifest；
- 正式公钥、专用测试购买 License；
- PSD probe 样本子集及畸形样本；
- 空白用户数据目录、空 keyring 测试账户。

### 8.2 主剧本

| 步骤 | 操作 | 必须结果 |
|---|---|---|
| 1 | 断网首次启动，扫描 PSD | 内置 Catalog 识别；`AvailableUninstalled`；生成 task；主 generator 无错误 |
| 2 | 打开网格 | 锁占位；不发送普通 thumbnail 请求 |
| 3 | 联网刷新 Registry | 签名/sequence/expiry 通过；市场显示当前 target 包 |
| 4 | 打开购买页 | 仅打开后端 Catalog 给出的 HTTPS 商店域名 |
| 5 | 下载并安装 | `.part`→验 hash→读签名清单→安全 staging→平台签名→原子切换→健康握手 |
| 6 | 输入有效 token | keyring 保存；状态 Authorized；Coordinator 自动 wake |
| 7 | Pipeline 处理 | task processing→done；WebP 原子落盘；DB/layout/event 一致 |
| 8 | 网格刷新 | 占位变缩略图；CLIP/face 此后才可领取 |
| 9 | 重启 | 安装、授权、缩略图命中；空闲无 Worker |
| 10 | 修改 PSD（确认 mtime 已变更，R6） | cache_key 变化；旧 task/产物失效；自动重做 |
| 11 | 升级 Worker | 指纹变化；安全 quiesce/切换；受影响 task 重做 |
| 12 | 卸载 | Worker 全退出；License 按选择保留；媒体与产品状态准确 |

### 8.3 失败剧本

- 断网/下载中断：可续传；旧安装仍可用；
- token 错误/过期：不覆盖旧有效 token；
- 包 hash/签名/平台签名错误：不触碰 current；
- 安装切换后健康检查失败：自动回滚；
- Worker crash/timeout：重试、熔断、UI 错误明确；
- 源文件处理中变化：旧响应被丢弃；
- App 强退：下次 lease 恢复，无永久 processing；
- 磁盘满/DB commit 失败：无半成品引用。

端到端结果写 `plan-docs/exotic_format_plugin_plan/exotic-psd-release-e2e.md`，记录 Host/Worker/package sequence、样本 hash、平台、结果、日志定位，不记录 token。

## Phase 9 — 发布硬化

### 9.1 安全门禁

- 签名 Registry、package manifest、全文件 hash、平台签名全部接线；
- ZIP 攻击集与 IPC 攻击集进入 CI；
- 插件目录与 Catalog asset 暴露最小化；不对前端开放 Worker 可执行文件；
- Worker 环境变量使用 allowlist，移除 token、代理凭据及无关敏感变量；
- source path 只传当前 task 所需文件；日志自动脱敏；
- v3 只接受厂商发布 key，未知 vendor key 拒绝；
- 明确安全声明：厂商 Worker 是受信任原生代码，不是 sandboxed plugin。

### 9.2 资源与稳定性

- Windows 使用 Job Object：kill-on-job-close、进程数、内存限制；
- macOS 使用可行的 rlimit/监督采样与超限 kill，记录实际约束差异；
- task timeout、全局并发、单插件并发均由 Host 强制，manifest 只能降低不能提高全局上限；
- 连续崩溃熔断；stderr 有界；子进程退出必 wait，禁止 zombie；
- staging/tmp/backup/孤儿缓存均有启动恢复与保留期清理；
- task lease owner（`claimed_at` + `lease_owner` + ttl 过期回收）方案完成，避免双实例重复领取；项目无单实例插件，不以「单实例」兜底（R2）。

### 9.3 性能门禁

固定硬件、固定语料、相同 Release 配置重复至少 5 次，报告中位数与离散度：

| 指标 | 门槛 |
|---|---|
| 无 exotic 命中的 10 万常见媒体扫描吞吐 | 相对基线回退 ≤2% |
| 常见缩略图吞吐 | 相对基线回退 ≤2% |
| Catalog/Host 冷启动增量 | ≤20 ms |
| 空闲内存增量 | ≤10 MiB；无 Worker 进程 |
| 100 万普通 + 100 exotic 的 ready query p95 | ≤50 ms |
| 用户持续滚动 | Exotic 停止新派发；前台帧时间不出现可归因长尾 |
| 多插件并发 | 总 Worker 数永不超过全局上限 |

“无可感知下降”不得作为唯一验收描述。基准脚本、数据生成种子、原始结果入仓或附发布报告。

### 9.4 自动化测试矩阵

```text
单元：Catalog / task / resolution / fingerprint / license / package path / frame
集成：scan→task、router、AI/face/derive gate、coordinator wake、sink
故障注入：crash/timeout/short-write/disk-full/DB-fail/app-kill
攻击：registry rollback、zip-slip/link/extra-DLL/zip-bomb、IPC oversized/corrupt
前端：全部 availability/task state、零 common request、事件乱序
性能：常见格式基线、百万库 SQL、全局并发
平台：Windows x64、macOS arm64；如声明则追加 Windows arm64/macOS x64
```

关键正式路径不得只留 GUI 手测。至少提供一个后端集成测试，使用测试 key + 本地 HTTPS/受控 HTTP fixture 完成下载、验签、安装、激活、处理；Release key 与测试 key 严格分离。

### 9.5 跨平台发布

Windows：

- Authenticode 发布者匹配；
- `CREATE_NO_WINDOW`；
- 路径含空格/Unicode/长路径；
- Job Object 与进程树回收；
- Defender/SmartScreen、升级文件锁实测。

macOS：

- Developer ID、hardened runtime、notarization、stapling；
- `codesign --verify`、`spctl`、干净机器 Gatekeeper；
- arm64 原生 Worker；若提供 x64/Rosetta，作为独立 target 包；
- 可执行位、路径 Unicode、App 更新后插件仍可启动。

缺当前平台包时 Catalog 返回 `UnsupportedPlatform`，不能回退下载其他架构二进制。

### 9.6 可观测性与隐私

结构化指标：任务领取/完成/错误、Worker 启动/退出/超时、队列深度、熔断、安装阶段耗时。日志使用 plugin_id、task_id、错误码；绝对文件路径默认 hash/截断，禁止 token、subject、下载授权参数。

提供“导出诊断”时先展示将导出的字段，并排除源媒体、License token、keyring 内容和完整本地路径。

### 9.7 文档回写

- `architecture_notes.md`：三份真相、任务指纹、Router 边界、跨流水线门控、优先级；
- 用户文档：受支持 PSD 范围、购买/激活、Worker 信任说明、卸载与 License 保留；
- 开发文档：Catalog 发布、package 签名、key 轮换、回滚、SBOM；
- 发布记录：probe、E2E、安全攻击集、性能、Windows/macOS 签名结果。

## 第二格式验收

PSD 发布后，选择一个 probe 成本可控的第二格式。新增内容只允许：

1. Catalog offering；
2. Worker 与 package manifest；
3. License SKU/商店条目；
4. capability 对应的通用 Sink 已存在时零 Host 业务特判。

若 scanner、Host、Router、前端按具体格式新增 `if format == ...`，视为 v3 扩展契约失败。若新增的是全新 capability/Sink，则允许扩展协议与通用能力处理器，但仍禁止格式特判。

## 发布清单

- [ ] 前端全状态、能力感知、零普通 thumbnail 请求
- [ ] 正式 PSD E2E 主剧本与失败剧本有记录
- [ ] Security/IPC/ZIP 攻击集 CI 通过
- [ ] Coordinator 竞态、lease、熔断、恢复通过
- [ ] 常见格式与百万库性能门禁通过
- [ ] Windows/macOS 签名、公证、干净机器启动通过
- [ ] SBOM/许可证/合规审核/可复现 hash 齐全
- [ ] Release 无 dev bypass、测试 key、token 日志
- [ ] 架构、用户、开发、发布文档回写

## 全集完成

Part1–Part4 全部 DoD 通过，才可标记 PSD 插件“可发布”。任一签名、原子安装、自动唤醒、跨流水线门控、缓存失效或性能门禁未完成，只能标记内部预览。

## 续作提示词

```text
实施 plan-docs/exotic_format_plugin_plan/exotic_format_plugin_v3_part4_frontend_release.md。
前置：Part1–Part3 全部完成。
先补前端状态与自动测试，再跑正式 Release E2E，最后执行安全/性能/平台门禁。
不得以 GUI 手测替代关键自动测试；每阶段中文 commit。
```
