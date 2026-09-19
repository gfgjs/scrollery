---
status: 快照
type: 工作记忆
line: 扫描线P1修复与tmp吸收
created: 2026-08-23
---

# 任务计划:扫描线P1修复与tmp吸收

## 目标
在 dev(845b75a)上修复 2026-08-21 扫描性能线遗留的 P1 跨根并发误标缺陷,并移植 tmp 对照线(scrollery-tmp-23a5679 @ 207af98)中验证过的优秀实现:富化选批队列表、walker 单次 stat、XMP 首字节定位;最后复跑 clippy/测试并收口文档。

## 当前阶段
已完成(收口 2026-08-23)

## 阶段

### 阶段 1:seen 表按根隔离(修 P1)
- [x] `_mm_seen` → `_mm_seen_r{root_id}`,`init_seen_table`/`insert_seen_id` 带 root_id
- [x] `mark_missing_preloaded`:表缺失保守返回 0;差集后 DROP 本表(解决 tmp 线的常驻内存缺陷)
- [x] fast_scan 流式写入路径与 QuickDirPruner 回填路径同步改造
- [x] 测试:跨根隔离、缺表保守跳过、用后销毁语义、既有生命周期用例迁移
- **状态:** done(提交 6c275e7)

### 阶段 2:富化选批队列表(folder/filename 序脱离 O(N²))
- [x] 非 keyset 序(folder / *+filename)建一次性排序 TEMP 队列表,按 seq 游标续取
- [x] 队列表名带调用代次后缀,规避 tmp 线的同根重扫 DROP 竞态;任务结束 DROP 自有表
- [x] keyset 判定沿用 scrollery 谓词(已正确排除 none+filename,不引入 tmp 的 keysetable 错判)
- [x] 保留既有 fallback 兜底轮语义(并发插入 + 失败重试)
- [x] 测试:queue 序与全量视图序对拍(含 folder tree_sort_key 真实编码)、EXPLAIN 计划锁保持
- **状态:** done(提交 226e7c5)

### 阶段 3:walker 单次 stat + XMP 首字节定位
- [x] walker 文件分支 `file_type()`+`metadata()` 双调用合并为单次 `symlink_metadata`(跨平台常数项)
- [x] `contains_bytes` 改首字节定位 + 整串比对(免全程滑窗)
- **状态:** done(提交 1692fd5)

### 阶段 4:验证与收口
- [x] 当前 stable(1.98)复跑 cargo test/clippy,vue-tsc
- [x] 查 CI 基线状态:dev 自 2026-08-20 起既有红(runner build.rs 环境/Linux 排队超时/docs-governance 既有文档),与本线无关 → 登记 F-006 不在本线修
- [x] todo.md 增补(含 1077→1094 计数更正)、experience §51、Spec02 seen/选批/walker 细节同步
- [x] 三件套收口迁 worklogs + README 登记
- **状态:** done

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| seen 表按 root 后缀隔离 + finalize 后 DROP,而非加扫描互斥 | 互斥改变调度语义且治标;分表是 tmp 线已验证的方案,DROP 补上其内存常驻缺陷 | D-001 |
| 队列表带代次后缀,不复刻 tmp 的固定表名 | tmp 线存在旧任务 DROP 新任务表的窄窗口竞态(审计实证),代次隔离 ~5 行成本根除 | D-002 |
| queue 模式融入现有批循环,不整体重构为 ImageBatchSource 结构 | 最小 diff 达成同复杂度收益;对拍测试按现有 query_batch 风格驱动 SQL 函数 | D-003 |
| 不移植 live photo 分片流式与前端防抖调整 | 前者涉及 SELECT 游标中 UPDATE 的隐性约束需单独设计,后者参数未真机验收;登记候选另行立项 | D-004 |
| CI 基线红登记不修 | 三重既有原因均与本线无关(runner 环境/排队超时/既有文档);修 11 条基线 lint 不能使 CI 转绿,徒增无关 churn | D-005 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| execute_batch 执行含 ?1 的灌入 SQL | 静默把未绑参数当 NULL,队列表空 | 拆 CREATE(batch)+INSERT(prepare 绑定),注释与测试双锁 |
| QueueGuard::new 的 Self 生命周期绑定 impl 参数 | 编译错 lifetime may not live long enough | 返回类型显式 QueueGuard<'a> + 构造字面量 |
