---
id: 2026-09-12-文件夹入库元数据流水线性能调查-findings
status: snapshot
type: working-memory
line: 入库扫描元数据读取性能优化
created: 2026-09-12
---

# 发现：文件夹入库元数据流水线性能调查

## 需求
调查文件夹入库、元数据扫描建档流水线，分析性能瓶颈。

## 发现
- 已存在入库扫描元数据读取性能优化与扫描线P1修复状态分片；历史结论需对照当前代码。
- 工作区已有其他会话的文档和暂存改动，保留原样。

## 耐久提升候选
| 候选 ID | 内容摘要 | 建议去向 |
|---|---|---|

## 主会话核验：可观测性与边界
- 基线 HEAD：33f54140；已有其他会话文档改动，不应混入本轮。
- `src-tauri/src/ipc/scan_commands.rs:623-627,839-858` 已有完整流水线 span，以及 fast_scan_ms、enrichment_ms、orchestration_ms、total_ms；不能说完全没有耗时观测。
- `src-tauri/src/scanner/enricher.rs:477-499,906-917` 已有 EXIF 路径计数、工作池规模和富化总耗时；尚无逐批 header/read/parse/DB wait/DB hold 分布。
- `scripts/acceptance/isolated-app-smoke.mjs:1530-1558` 的 full-scan 验收等快速扫描 completed 并核对条目数，不能据此推出全部元数据已完成或大库吞吐率。
- `src/perf/performanceRecorder.ts:8-22` 的固定 span 覆盖画廊/时间轴；不提供后端扫描分段统计。
- `src-tauri/src/state.rs:1052-1081`、`src-tauri/src/derive/pipeline.rs:434` 已使派生和 AI 对扫描让步，不能无依据将 AI 竞争列为主因。

## 实测方案草案
分首次入库、无变化 quick、完整重扫、1%变化与失败文件；对普通照片、超大/复杂格式和混合音视频分组。单根先测，再多根及浏览同时运行；记录首批可见、fast结束、富化结束、每格式成功率、DB锁等待/持有P95、批内慢项、实际读取字节与逻辑文件大小。冷暖缓存分开，不将进度累计文件大小视为物理I/O量。
## 收口候选
| 候选 ID | 内容摘要 | 建议去向 |
|---|---|---|
| F-001 | 快扫与富化批次锁步及混合媒体阶段等待 | todo |
| F-002 | 1000项头读保留句柄/字节及慢格式尾延迟 | todo |
| F-003 | 扫描期全表统计、布局取数与富化尾沿防抖 | todo |
| F-004 | 目录收尾autocommit与Live Photo全根对账 | todo |
| F-005 | quick线性seen工作、自动模式会话策略与条件指纹读取 | todo |

全部候选已落滚动状态与有证据的审查报告；不将阶段等待直接等同最大耗时占比。子代理初稿中的‘全流程单线程’、‘扫描与富化刷新频率叠加’、‘每次布局必定未命中’与不精确的SQL样本计数均需经过主会话纠正后才入报告。
