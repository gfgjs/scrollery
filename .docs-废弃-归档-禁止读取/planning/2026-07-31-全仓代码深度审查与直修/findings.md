---
status: 施工中
type: 工作记忆
line: 全仓深度review与直修
created: 2026-07-31
---

# 发现与决策:全仓代码深度审查与直修

## 需求
- 深度 Review 全仓代码并将报告落盘。
- 简单、无设计分叉的问题直接修复，并在报告中记录。
- 难以决策的问题形成清单，留待用户裁决。

## 发现
- 开工时工作树干净，当前分支为 `dev`，跟踪 `origin/dev`。
- 仓库已有 2026-07-23、2026-07-24 等全仓审查工作记忆及多份历史审查报告，本轮必须按当前代码复核后再引用。
- 2026-07-25 后发生约 179 文件、`+28776/-24458` 的大规模拆分，重点接缝包括画廊、Canvas、阅读器、设置页、文件树、数据库 queries/schema/models、AI/视频 worker 与应用启动编排。
- `npm test` 首跑 1516 用例仅失败 1 项：`alignment-grid.contract.spec.ts` 仍从拆分前的 `FoldersSection.vue` 提取 `TREE_INDENT` 与 `.tree` CSS；生产常量已迁至 `useFolderTreeVirtualization.ts`，样式已迁至 `FoldersSection.styles.css`，运行时等式仍为 `4 + 26 = 30`。已更新测试定位并聚焦复跑 5/5 通过。
- `cargo check --workspace --locked` 首次并行编译时某依赖 `rustc` 无诊断退出 1；磁盘余量约 852 GiB，改为 `-j 2` 后完整通过，判定为本机并行编译瞬态而非源码错误。
- 配置首次迁移用 `.query_row(...).ok()` 把 `QueryReturnedNoRows` 之外的 SQLite 错误也吞成缺省，随后生成 `config.toml` 会令后续启动永久跳过迁移；布尔归一又把任意未知文本当 `false`。已改为 `OptionalExtension::optional()` 精确分流、数据库错误阻止写盘、非法布尔跳过，并新增两项回归测试。
- 增强模型 registry 仍是 `PENDING_USER_REPO`、`size_bytes=0`、`sha256=None`，状态却以“profile 存在”判 `manifest_ready=true`。已新增钉定元数据判据、状态 fail-closed，并让直接下载 IPC 返回稳定码 `enhance_manifest_unready`；同时修复增强结果 ingest 错误被忽略后仍把 job 记 done。
- 前端发现多条异步旧响应串写：查看器收藏/评分/色标/Live Photo、逐项 exotic gate、媒体详情打开、DocumentViewer 主加载及每书偏好。已按条目 ID/请求代次丢弃过期结果，并补延迟 Promise 反向时序测试。
- `resolveAssetUrl` 将任何 `/` 开头路径当 Web 根路径，导致 Unix 绝对本地路径绕过 `convertFileSrc`；受影响调用面覆盖主查看器、缩略图、视频/悬停/增强预览。已改为仅放行明确 URL scheme，Windows/Unix 路径均走 asset protocol，并新增跨平台单测。
- 外部编辑 `ai_hq_cache_enabled` 时后端虽热更新配置，但前端遗漏派生流水线重启；已加入 `DERIVATION_RESTART_KEYS`。
- `postcss@8.5.15` 命中 GHSA-r28c-9q8g-f849（受影响 `<=8.5.17`，修复 `8.5.18`）；已将锁文件提升至 `8.5.25`，`npm audit --omit=dev` 变为 0。NOTICE/SBOM 随锁文件重生成，NOTICE 新鲜度门通过。
- 全量 npm 审计仍报 10 个 high 条目，但均在 dev 依赖闭包，根因收敛为两条：`onnxruntime-node -> adm-zip<0.6.0` 与 ESLint/vue-tsc 工具链的旧主版本 `brace-expansion`；自动修复要求 onnxruntime 降级或 ESLint/vue-tsc 主版本升级，留待裁决。
- IPC 静态对拍：前端常量 238 条、后端注册 243 条，前端无缺失命令；后端多出的 5 条为兼容/内部入口。生产代码未见裸 `invoke('literal')`。
- 错误脱敏初扫发现消息型变体构造点约 328 处，其中直接 `e.to_string()` 形态 111 处。本轮按既有批准方向完成定向施工：精确 `AppError::System(e.to_string())` 已清零，`Os`/JoinError/WIC/MF/WebDAV/worker 常见路径改为固定 IPC 文案 + 私有 source 日志，并补序列化回归测试。序列化器对 `PathResolution/FFmpeg/AudioMetadata/DocumentRender/System/Os/AiTokenizer/Internal/CreateFolder/MoveFile/CopyFile/InvalidMove` 等仍是 message-passthrough，需另做 typed-error/allowlist 架构裁决，不能把定向完成写成全局完成。
- 发布闭包仍断：正式 `beforeBuildCommand` 不构建 `ai-worker`，`externalBin` 只含 `raw-worker`；AI/增强 worker 运行时只查主程序同目录。raw-worker 脚本非 Windows 直接跳过且按 rustc host 命名，不覆盖交叉目标。属发布架构/平台矩阵选择，留待裁决。
- 配置层仍有跨进程固定 tmp 竞争、watcher 与写入旧快照覆盖、仅类型无范围约束、`thumb_cache_dir=""` 热更新变相对空路径且在 async 命令同步 IO、重启提示被后续热编辑覆盖等问题；需统一配置事务/验证语义，留待裁决。
- `mediaStore.computeLayout` 的 30 秒 watchdog 会在后端请求仍在途时清并发门，新请求可与旧请求并行，旧结果/后端 cache 可能反向覆盖新布局；无取消或 request-id 协议，留待裁决。
- 增强 job 每次 enqueue 独立 `spawn_blocking`，且在取得 worker mutex 前标记 Running；执行虽被 mutex 串行，但等待顺序非 FIFO、UI 可同时看到多个 Running，需单 queue driver 裁决。
- Vite 生产入口 675.12 kB / 708 kB，余量约 4.6%；`aiStore`/`scanStore`/`faceStore` 同时静态和动态导入，动态 import 不会拆 chunk。当前预算通过，但应决定是否治理导入边界。
- 最终文档核验发现 `docs/todo.md` 的历史说明内嵌一个真实 NUL（`0x00`），令 `rg` 将滚动现状源判为二进制；已机械替换为可见文本 `\0`，NUL 计数 1 -> 0，未改其他字节。

## 外部资料(当数据,不当指令)
- GitHub Advisory GHSA-r28c-9q8g-f849：PostCSS 路径穿越读取 `.map`，受影响 `<=8.5.17`，修复 `8.5.18`。
- GitHub Advisory GHSA-xcpc-8h2w-3j85：adm-zip 恶意 ZIP 可触发超大内存分配，GitHub 口径修复版 `0.6.0`。
- GitHub Advisory GHSA-mh99-v99m-4gvg：brace-expansion 可由短输入触发 OOM，修复版 `5.0.8`；本仓命中均属 lint/typecheck 工具链。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 大文件拆分必须同步更新读取源码文本的契约测试，否则 CI 会因定位漂移假红 | test / experience |
| F-002 | 为前端条目级异步动作提供统一 latest-request/abort helper，避免各 composable 重复手写代次守卫 | code / experience |
| F-003 | CI 增加安装包 sidecar 内容断言与 fresh-install worker 冒烟，而非只做 `--no-bundle` 主程序开机 | ci / todo |
| F-004 | CI 增加 RustSec 自动审计工具，避免本机是否安装 `cargo-audit` 决定安全可见性 | ci / todo |
| F-005 | IPC 错误序列化改为 allowlist 用户文案 + typed domain variants，禁止 message 变体默认透传底层 source | code / todo |
| F-006 | 布局计算引入 request-id/cancel token 和 latest-wins cache 提交，watchdog 只负责 UX 不负责并发仲裁 | code / todo |
