---
status: 快照
type: 工作记忆
line: 入库扫描元数据读取性能优化
created: 2026-08-21
---

# 任务计划:入库扫描元数据读取性能优化

## 目标
按已采纳的性能分析结论,分阶段消除「入库扫描-元数据读取」链路的结构性瓶颈,每阶段验证后独立 commit。

## 当前阶段
已收口

## 阶段

### 阶段 1:富化选批降复杂度
- [ ] V24 迁移新增 `idx_media_type_sort(media_type, sort_datetime DESC, id DESC) WHERE is_deleted=0`
- [ ] 图片 enrichment 默认 date+datetime 路径改 keyset 分页,并补 `id` tiebreaker
- [ ] 视频/音频 `*_needing_meta` 查询改 `m.id > ?` keyset + 对应 `(media_type, id)` 索引
- [ ] 测试:迁移到 V24、keyset 与旧查询同集等价、`EXPLAIN QUERY PLAN` 使用新索引不 TEMP SORT
- **状态:** done

### 阶段 2:DB 写热路径与索引瘦身
- [ ] `upsert_fast_scan_item`/`upsert_directory`/enrichment 写回循环使用 prepare_cached 或预 prepare
- [ ] V25 迁移删除冗余 `idx_media_directory`(隐式 UNIQUE(directory_id,file_name) 已覆盖)
- [ ] 快扫热路径减少 per-file 字符串分配(cache_key / rel_path 规范化)
- [ ] 测试:既有扫描 upsert、迁移、查询计划回归
- **状态:** done

### 阶段 3:增量重扫 quick 接线与剪枝前置
- [ ] 前端 `startScan` 支持并传递 `quick`(首次全量、后续重扫 quick)
- [ ] `MediaWalker` 暴露目录进入事件或提供目录级剪枝回调,使未变目录在 `metadata()` 前跳过
- [ ] 保持红线:快照只读、seen 回填、walk_complete 门闩、读不到 mtime 保守不剪
- [ ] 测试:quick 剪枝跳过 per-file stat、增量重扫不误标 missing
- **状态:** done

### 阶段 4:seen 集与 mark_missing 流式化
- [ ] 扫描期把 seen id 边扫边写入连接级 `_mm_seen`,取消 HashSet 双份持有
- [ ] `mark_missing` 提供 seen 已装载入口并保留清空语义
- [ ] 测试:同连接复用不清不误标、百万级规模正确性
- **状态:** done

### 阶段 5:元数据读取常数项优化
- [ ] fast_scan eager 尺寸读取复用 HeaderBuf(JPEG 少一次 open)
- [ ] XMP Motion Photo 检测改为字节搜索,免 `String::from_utf8_lossy` 128KB 分配
- [ ] 头缓冲按扩展名阶梯读取,保留截断回退
- [ ] 测试:方向一致性、XMP 标记窗口语义、缓冲回退
- **状态:** done

### 阶段 6:文档与收口
- [ ] Spec02 性能相关小节同步 as-built
- [ ] docs/todo.md 回写
- [ ] 三件套收口(closeout + 迁 worklogs + worklogs README 登记)
- **状态:** done

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 富化选批优先 keyset+索引,而非一次性队列 | 默认 date+datetime 是主路径,零额外内存;folder/date+filename 暂留原查询 | D-001 |
| `idx_media_directory` 判为冗余可删 | UNIQUE(directory_id,file_name) 的隐式索引同前缀,减少插入写放大 | D-002 |
| quick 仍保留 opt-in,前端重扫默认传 true | 就地编辑是已知漏检边界,需产品后续提供周期全量兜底 | D-003 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
