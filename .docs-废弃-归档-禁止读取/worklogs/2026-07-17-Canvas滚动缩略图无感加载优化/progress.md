---
status: snapshot
type: working-memory
line: Canvas滚动缩略图无感加载优化
created: 2026-07-17
---

# 进度日志:Canvas滚动缩略图无感加载优化

## 会话:2026-07-17
- 做了:读取项目文档治理契约，确认本任务按 planning 三件套执行；发现既有 Canvas 性能优化 worklog。
- 做了:定位到固定 96/48 预取预算、2000 条 LRU 上限和顶部优先入队三项 60px 密度放大点；实现按可见格数自适应的前后预取预算、滚动方向交换、滚入边缘优先和 12000 条/512MB 双约束缓存。
- 做了:为 browser UI harness 增加仅 DEV 的 `items` 密度参数；显式传参时使用 60px 行高并重复样本构造数千项，默认 18 张/220px 视觉场景保持不变。
- 做了:4K/5000 项首轮基准识别出 SVG fixture 失真（8192 次 Image fallback、缓存仅 194 格）；密度场景改用小型 raster，默认视觉 SVG 不变，待重跑有效基准。
- 做了:有效 raster harness 在 3840×2160、DPR 1.5、60px、5000 项下确认 Canvas 接管且 DOM 卡片为 0；预热后缓存稳定保留 5000 张（14.6MB），标准往返无重复 bitmap/fallback，draw P95 9.4ms；DEV/120Hz 整体 41.2 FPS，不把它夸大为 120Hz 绘制达标。
- 用户复验:第一版对已加载区域明显减轻、接近无感；全新区域仅略有改善；极速滚动出现“先顿一下、再追帧跳过一段”的严重回归，慢速随滚随出、极速停下才出图。
- 根因:快滚 gate 放行后，自适应整屏预取在同一 `draw()` 同步创建数千个 fetch/decode；主线程任务创建阻塞后才消费积压滚动输入。
- 做了:可见区与预取共用 64 个全局在途槽；视口外预取改成可取消、可续跑的 iterator，每个 idle slice 最多 64 项/2ms，滚动、行集、resize 或 gate 变化立即作废旧计划；失败也触发补槽重绘，避免全失败尾波停滞。
- 做了:核对缩略图规格契约。项目已有 120/240/480/960 档和分桶路径；480 源会增加冷区读取/完整解码成本。极密模式应按格子长边×DPR 自适应复用 120/240 档，不新增 60 档，固定 120 在 DPR 2 宽图上也可能糊。多档方案尚需读取选路/回退与性能 A/B，不在本轮未经测量直接改后端。
- 验证:分片 iterator 与 loadingCount 新增回归钉；定向 Vitest 2 文件/71 测试、typecheck、lint、生产 build 均通过；build 仅报告既有 mixed import，入口 578.32/620kB。
- 用户复验:C 版冷区几乎无改善，仅把固定上→下的替换改成随滚动方向；请求优先级方案判定无效，视觉方向反转撤销。
- 做了:D 版把快滚 gate 从全关改为“最新视口、仅现成 status=1 WebP、最多 8 个在途”的小通道，不在飞掠中触发后端生成；正常态仍为 64 槽。
- 做了:D 版对生成 WebP 解析 30B VP8/VP8L/VP8X 文件头，直接单次 `createImageBitmap(blob,crop,resize)` 到设备像素桶；非 WebP、原图直显和不支持该 overload 的 WebView 保留既有两段式兜底。
- 验证:D 版定向 Vitest 2 文件/75 测试、typecheck、lint、生产 build 通过；build 仅既有 mixed import，入口 578.32/620kB。
- 用户复验:D 版冷区无改善，极速无顿挫。判定 64 在途上限+idle 分片有效消除任务风暴；快滚 8 槽和单段 WebP 对冷区无感，撤销。
- 做了:最终代码只保留有实测收益的极密 LRU/字节双约束、自适应方向预取、可取消 idle 分片和 64 全局在途上限；冷区优化边界明确转交 120/240 多档源，不再在 Canvas 调度层换加载形态。
- 验证:`npm.cmd test -- --run src/components/media/mediaGridCanvas.helpers.spec.ts src/components/media/canvasThumbState.spec.ts` 通过 2 文件/68 测试；`npm.cmd run typecheck`、`npm.cmd run lint`、`npm.cmd run build` 通过。build 仅有既有 mixed import 报告。
- 验证:最终候选全量 `npm.cmd test` 通过 88 文件/1161 测试；`npm.cmd run typecheck`、`npm.cmd run lint`、`npm.cmd run build` 全部通过。build 仅有既有 mixed import 警告。
- 结论:用户真实图库终验为“冷区无改善，极速无顿挫”；Canvas 调度层以热区接近无感、极速无顿挫收口，480px 冷区转入 120/240/480 多档源独立任务。
- 验证:`git status --short` 无输出，开工时工作树干净。
- 代码提交:`384e10e`（`perf(canvas): 限制极密缩略图任务风暴`）。

## 回顾(收口时填)
- 亮点:以用户三轮真实图库 A/B 为裁决，保留热区驻留与消除顿挫的有效改动，及时撤销只改变加载动画形态的实验。
- 教训:扩大预取覆盖不能在滚动 gate 放行时同步创建数千任务；窗口大小、遍历分片和全局在途数必须同时受控。
- 意外:快滚 8 槽通道与 WebP 单段缩放都未改善 480px 冷区，说明该场景的主约束已转到输入字节与解码像素，而非 Canvas 请求顺序。
