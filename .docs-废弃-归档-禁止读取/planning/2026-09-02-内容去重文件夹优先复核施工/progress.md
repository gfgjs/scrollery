---
status: 施工中
type: 工作记忆
line: 去重功能全局方案
created: 2026-09-02
---

# 进度日志:内容去重文件夹优先复核施工

## 会话:2026-09-02
- 做了:读取项目约束、docs 文档契约与文件夹优先复核方案；确认现有去重实现以精确重复组为主；建立施工三件套。
- 做了:完成动态文件夹统计/候选/树层/范围画廊/比较成员 IPC；新增 opaque `viewStamp`、保护/来源/物理别名投影；以父链上溯替换目录到后代的递归聚合；新增纯计划生成器、推荐描述符、include/exclude override、preview/apply 原子软删除；替换去重页为文件夹优先工作区并接入虚拟画廊、缩略图、键盘、响应式与回收站入口。
- 修复:修正统计列位、SQL 拼接空格、过滤画廊 keyset 续页、空选区 preview、切换文件夹/清空草案的迟到响应，以及主题幽灵 token；增加 physical alias 默认只复核规则。
- 验证:`cargo fmt --all -- --check`、`cargo check --workspace --locked`、`cargo clippy --workspace --locked -- -D warnings`、`cargo test --workspace --locked` 通过；`npm test` 为 146 个文件/1635 个测试通过；`npm run lint`、`npm run typecheck`、`npm run build`、路径卫生、主题色板/对比度、渠道 bundle 扫描与 `git diff --check` 通过。
- 遗留:100K/1M 聚合基准、候选树真正懒展开/树虚拟化、真实 Live Photo/卷/移动端手验与 P4 收拢仍待下一批；`node scripts/generate-notice.mjs --check` 被既有 NOTICE/lockfile 许可计数漂移阻断，未改无关依赖清单。

## 会话续作:2026-09-02
- 做了:新增 `dedup_folder_bench` 合成基准，覆盖 100K/1M 媒体项、父子目录、稳定 checksum、SQLite 冷页缓存近似与进程 PeakWorkingSet 记录。
- 证据:100K 动态候选冷/暖约 1.47s；1M 动态候选约 16.45s/16.38s、根统计约 14.85s/15.52s，峰值约 983MiB，超过“首次聚合超过 1 秒”的约束。
- 做了:加入 `DedupFolderStatsCache`，缓存键含 `analysis_generation`、`data_version`、`hash_version`、筛选器；候选、根/子树分页、摘要/画廊范围存在性、preview/apply 统计读取统一复用快照，清库主动清槽。
- 做了:候选快照在 Rust 侧完成命中祖先闭包、整数比例优先级分页与目录 ASCII NOCASE 排序；100K checksum 与动态 SQL 相同，1M 缓存暖候选约 0.27ms；树 UI 已懒展开并使用 TanStack 虚拟滚动、roving focus 和方向键语义。
- 验证:缓存聚焦单测 3 项、原有去重查询单测 16 项、`cargo test --workspace --locked`（主库 1206 项，另含各 worker/协议 crate）通过；`npm test` 147 文件/1637 测试、typecheck、lint、生产构建、主题对比度/色板、渠道扫描、rustfmt、clippy 和 `git diff --check` 均通过。
- 修复:定位并修复 `/duplicates` 文件夹候选请求的 Tauri IPC 参数形状错误；候选、子目录、摘要和画廊结构体参数统一包装为 `{ request: ... }`，根目录参数保持 `{ filters }`。
- 验证:新增文件夹 store IPC 请求形状回归；`npx vitest run` 通过（148 文件/1638 测试），`vue-tsc --noEmit` 与相关 ESLint 检查通过。
- 遗留:快照首次冷构建仍受现有聚合 CTE 限制，计划组/范围画廊仍按目标范围动态查询；需真机验证首屏阶段反馈、Live Photo/卷/移动端与 P4 收拢；NOTICE 检查仍为既有许可计数漂移。

## 回顾(收口时填)
- 亮点:推荐草案保持后端解释、前端只保存少量 override；SQL 与纯函数共享同一安全选择口径；按媒体位置上溯父链避免了全目录后代配对。
- 教训:分页过滤必须在后端持续消费原始 keyset，不能用单页空结果代表没有匹配项；视图异步请求需要单独代际守卫，不能只依赖组件卸载。
- 意外:候选全量递归的初版虽然功能测试成立，但性能形态不符合设计约束，已在首批施工内改为位置×深度归并。
