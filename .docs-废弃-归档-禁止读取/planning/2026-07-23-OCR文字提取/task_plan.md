---
status: active
type: working-memory
line: OCR文字提取
created: 2026-07-23
---

# 任务计划:OCR文字提取

## 目标
图片查看器与视频播放器新增「提取文字信息」按钮:提取图片/视频当前画面内文字,结果用户可复制;技术方案经联网调研 + 仓内摸底后交用户确认,确认后才施工。

## 当前阶段
阶段 4:施工(B0 exotic 摸底在飞,architect 计划中)

## 依赖 DAG
P0 建三件套 → P1a 联网调研 ∥ P1b 仓内摸底 → P2 方案汇总+裁决点 → P3 用户确认(硬闸) → P4 施工(DAG 届时另画) → P5 收口(用户明示)

## 阶段

### 阶段 1:立项与并行摸底
- complete:researcher 14 轮落 research-web.md;摸底 12 轮落 recon-codebase.md。决定性发现=仓内 ort v2.0.0-rc.12 即调研最新版、视频截帧链路现成、PP-OCRv5 CJK 最优且 Apache-2.0。
- **状态:** complete

### 阶段 2:方案汇总与裁决点清单
- complete:三案对比(A 全原生 / B 统一 ONNX / C 混合)+ 推荐组合 B′(桌面 PP-OCRv5 ONNX 复用 ai-worker)+ 自研管线 + 首用下载 + 前端 canvas 取帧;4 裁决点已交用户(见 progress 前情)。
- **状态:** complete

### 阶段 3:用户确认(硬闸,不过闸不施工)
- complete:4 裁决点全拍板(2026-07-23 用户答复):B′ 引擎 / ai-worker 自研+付费插件 / 可选档位下载 / 前端 canvas。
- **状态:** complete

### 阶段 4:施工(确认后画施工 DAG,分批施工+随批复核)
- B0 摸底 exotic 插件门控+商店接线+aiStore 下载列表 — done(recon-exotic.md)
- T1(原 B2 协议片)exotic-protocol OcrBatch 协议族 — done(commit 1)
- T3(原 B5 门控片)builtin offering 门控语义 + OCR 付费插件条目 — done(commit 2)
- T4(原 B2 扫描片)fast_scan/walker 播种清理 — done,待随 T2 提交
- T2 ai-core OCR 管线(det+cls+rec 预处理/推理/后处理+CTC 解码+字典)— done(落地22轮22测绿),待深审闭环
- T5、T6 在飞
- 波次 3-5(T7 起)未开工
- 细粒度任务卡=construction-plan.md T1-T11(D-OCR-1..7 从属决策在其 §0)
- 施工全闭环(T1-T11)+ 资产 URL/sha256 已回填 ocr_registry(2026-07-23,ModelScope 主源本机实测,见 model-assets.md 追记段);剩余=①模型落位后 golden 对拍定案 swap_rb+ocr_bench 实测(README 定案清单);②GUI 手测清单(见 progress);③商店 storeUrl 占位随域名统一替换(仓既有事项)
- **状态:** in_progress

### 阶段 5:收口(仅用户明示)
- **状态:** pending

## 关键决策
<!-- 需收口提升的决策编 D-001 递增填「候选 ID」列;仅会话内有效的留空 -->
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 引擎路线=B′:桌面 PP-OCRv5 ONNX 复用 ai-worker;macOS/iOS/Android 后置按端另裁 | CJK 最优+基建复用最高+Apache-2.0;Windows 原生恰最弱(旧 API 无质量数据、OneOCR 无官方 API) | D-418 |
| 载体=ai-worker+自研管线进 scrollery-ai-core(oar-ocr 仅参考);产品形态=插件商店付费插件 | 与 CLIP/人脸同构复用 SessionPool/DirectML/registry;维护风险收敛;付费口径用户定 | D-419 |
| 模型分发=首用下载+设置页预下载;参考 AI 分析下载列表做多档位可选(mobile/server) | 包体不涨;复用 download_model+Channel 机制;档位换精度/速度自由度 | D-420 |
| 视频取帧=前端 canvas 复用截帧链路 | 所见即所得(用户所见画面即 OCR 输入);改动最小 | D-421 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
