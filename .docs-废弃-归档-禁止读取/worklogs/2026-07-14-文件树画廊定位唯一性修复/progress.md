---
status: snapshot
type: working-memory
line: 文件树画廊定位唯一性修复
created: 2026-07-14
---

# 进度日志:文件树画廊定位唯一性修复

## 会话:2026-07-14
- 做了:建立任务三件套；确认工作树存在与本任务无关的用户改动，后续保持精确编辑与提交范围。
- 做了:追踪 `TimelineScrubber → MediaGrid.scrolledDirectoryId → FoldersSection → useFolderTree`；修正 SQL/内存 folder 唯一排序；新增 latest-wins 文件树同步队列及回归测试。
- 验证:`npm.cmd test -- --run src/components/sidebar/sections/folderTree.helpers.spec.ts` 通过（1 file，42 tests）；Rust 三项定向测试通过：唯一 rank、SQL/内存排序等价、同路径同文件名 filename 排序。
- 错误账:首次同名文件 SQL 测试失败 `no such collation sequence: NATURAL_CMP`；测试内存连接补用生产 `register_custom_collations` 后重跑通过。
- 做了:复核 latest-wins drain 生命周期，消除 Promise 外层 finally 的极窄丢请求窗口，并新增双 microtask 收尾竞态回归测试；定向测试最终 43/43。
- 验证:前端全量 `npm.cmd test -- --run` 通过（71 files，858 tests）；`npm.cmd run typecheck`、`npm.cmd run lint`、`npm.cmd run build`、`npm.cmd run check:contrast` 均通过。build 仅输出既有 mixed dynamic import 与大 chunk 警告。
- 验证:`cargo test --workspace` 全 workspace 无失败（主 crate 493 passed、5 ignored，其他 crate 与 doc tests 全绿）；`cargo fmt --all -- --check` 通过。
- 验证:`node tools/check_docs.mjs`、`node tools/check_plan_canonical.mjs`、`node tools/check_docs_index.mjs` 全通过。
- 提交:代码、测试与施工期记录已提交为 `c2536e6`（修复大库画廊联动文件树错位）。
- 遗留:当前环境没有用户的 40 万图生产库，真实大库拖动滑块的运行级复测仍需用户在 Tauri 应用内完成；自动化已覆盖同路径、同目录名、同文件名、深层异步乱序和 drain 收尾竞态。

## 回顾(收口时填)
- 亮点:没有停在表面的“同名查找”猜测，而是沿 id 链确认前端身份已正确，最终同时修复后端目录分组不唯一与前端异步晚到覆盖两条独立根因。
- 教训:分组 separator 的前提不只是排序稳定，而是每个真实实体必须连续成组；高频状态驱动的懒加载也不能用无约束 Promise 并发。
- 意外:latest-wins 初版仍有一个 Promise 外层 finally 的极窄收尾窗口；用双 microtask 回归测试复现后，将在途标记清理移入 drain 同步 finally 才真正闭环。
