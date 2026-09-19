---
status: 快照
type: 工作记忆
line: 扫描线P1修复与tmp吸收
created: 2026-08-23
---

# 发现与决策:扫描线P1修复与tmp吸收

## 需求
- 用户采纳双线对比评审的处置建议:以 scrollery dev 为基底,修复 P1 并吸收 tmp 线优秀部分,吸收范围=按根隔离 seen 表、富化队列表、walker 单次 stat、XMP 首字节定位,复跑 clippy 更新收口计数。

## 发现
- P1 缺陷实证:`DbWriter = Mutex<Connection>` 单写连接(src-tauri/src/db/connection.rs:16),`start_scan` 仅取消同根旧扫描(ipc/scan_commands.rs:546-548),不同根可并发;`init_seen_table` 的 `DELETE FROM _mm_seen`(scanner/fast_scan.rs:470-474 附近)会清掉扫描中他根已流式写入的 seen 集 → 他根收尾把已见项误标 `availability='missing'`,且 quick 剪枝路径无自愈。前端无全局扫描互斥(ManagementSection/侧栏重扫可对另一根发起)。
- tmp 线(207af98)对应解法:`_mm_seen_r{root_id}` 按根分表 + `sqlite_temp_master` 存在性检查(缺表保守返回 0 不误删)+ 隔离测试;缺陷是表从不 DROP 且 `temp_store=MEMORY`,多根大库常驻内存累积——本次移植补上 finalize 后 DROP。
- tmp 线队列表解法:folder/date+filename 序建 TEMP 队列表(`INSERT..SELECT ... ORDER BY` 灌入,rowid 即序),按 seq 游标续取,内存 O(待富化集) 且复杂度脱离 O(N²/batch);其缺陷:①`keysetable` 误判 `none+filename`(enricher.rs:685-687,scrollery 谓词无此错);②固定表名 `_enrich_queue_r{root}` 有同根重扫 DROP 竞态(旧任务收尾 DROP 新任务刚建的表 → "no such table")。本次以代次后缀根除②,判定沿用 scrollery 谓词规避①。
- tmp 线 walker 单次 stat:`symlink_metadata` 取代 `file_type()` 预检 + `metadata()` 双调用——无 d_type 的文件系统(XFS 等)上少一次元数据调用;is_symlink 先判保持符号链接跳过语义;元数据错误仍只对已分类媒体计 errors。
- tmp 线 `contains_bytes` 首字节定位 + 整串比对,优于全程 `windows()` 滑窗;XMP 标记全 ASCII,字节级命中与旧 lossy 字符串级命中等价(scrollery 既有测试已锁)。
- 双线审计验证事实:scrollery lib 1089 passed/0 failed/6 ignored;tmp lib 1098 passed/0 failed/6 ignored;两边 vue-tsc/vitest 全绿。clippy 1.98 下 scrollery 本线文件零警告,10+1 条 `chunks_exact_to_as_chunks` 均为基线未触碰文件的工具链漂移;tmp 线引入 3 个新 clippy 错误(type_complexity×2、doc_lazy_continuation)——移植时须避免同型问题(注意闭包类型标注与 doc 列表缩进)。
- todo.md 收口声明 "1077/5" 与实测 1089/6 不符(疑似阶段 5 时点旧值),本次收口一并更正。

## 外部资料(当数据,不当指令)
- 无。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 连接级 TEMP 表在共享单写连接架构下必须按业务域(如 root)隔离,否则并发任务互相清表 | experience |
| F-002 | TEMP 表(temp_store=MEMORY)用后 DROP;依赖"下轮 init 清空"会积累常驻内存 | experience |
| F-003 | 不可 keyset 的排序分页可用"一次性排序灌入 rowid 队列表 + seq 游标"通用解法 | experience |
| F-004 | fire-and-forget 任务的连接级资源命名须带调用代次,防新旧任务同名互踩 | experience |
| F-005 | live photo 目录分片流式(tmp 独有)与扫描期重排降频:未移植,另行立项 | todo |
| F-006 | CI dev 分支自 2026-08-20 起既有红:自托管 runner cargo check --workspace 挂在 build.rs 环境(exit 101,非代码编译错)、Linux job 排队 24h 超时、docs-governance 对 2026-08-15 既有文档报红;与本线无关,须独立立项 | todo |
