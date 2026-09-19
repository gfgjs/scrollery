---
status: snapshot
type: working-memory
line: WAL膨胀根治与启动积压驯服
created: 2026-07-13
---

# 发现与决策:WAL膨胀根治与启动积压驯服

## 需求
- 用户提交诊断稿:WAL 膨胀(全量生成写量堆积 + BSOD 跳过退出 checkpoint + 无 journal_size_limit)疑似导致每次开机 TRUNCATE 秒级阻塞;积压 resume 在冷启动关键路径饱和 CPU。方案 P1–P5。
- **用户明确:本轮交付=方案合理性分析,不直接施工。**

## 发现(全部已核实)
### 代码事实
- `src-tauri/src/db/connection.rs:26-34` PRAGMAS(写连接+每个读池连接共用):WAL / synchronous=NORMAL / cache=-64MB / foreign_keys=ON / busy_timeout=5s / temp_store=MEMORY / mmap=256MB。确无 journal_size_limit。
- **全库无人设置 `wal_autocheckpoint`** → SQLite 编译默认 1000 页(≈4MB)自动 PASSIVE checkpoint 生效:每次 commit 后 WAL 超阈值即自动回收。**原诊断「全量生成期间没有周期性 checkpoint」不成立**——周期 checkpoint 一直在场,只是没人显式写。
- `connection.rs:65-105` checkpoint_wal_at_boot:开机 TRUNCATE + 计时/体积探针(>500ms 或 >64MB 升 warn),lib.rs:393 在 tracing 就绪后、管线拉起前调用。
- `lib.rs:955-968` 退出钩子 TRUNCATE 仅优雅退出跑(BSOD/强杀不经过)——此点诊断正确,但后果被高估:非优雅退出残留的 WAL 上限≈auto-checkpoint 工作集,不是「整批写量」。
- `App.vue:318/326` ai/face maybeAutoResume 在 onMounted 即发(互斥靠后端 GPU 单持有者门闩 state.rs:117-131);**派生线不同**:useDerivationAutoStart(App.vue:258)=启动后 3s 延迟 kick + 先查 derivation_status + 后端让步纪律——「派生 resume 压在冷启动关键路径」不成立。
- `queries.rs:7223` synchronous=OFF 是 #[ignore] 基准测试灌数据,非生产路径。
- lib.rs:530 的 24h 任务确是 PRAGMA optimize(非 checkpoint),启动后 3 分钟首跑——诊断此点属实。
- 读池 r2d2:SQLITE_OPEN_READ_ONLY + min_idle(0)。

### 探针实测(2026-07-13 当日日志,唯一存留日志文件;C:\Users\gf\AppData\Roaming\com.scrollery.app\logs)
- 生产库 scrollery.db=23MB(在 **C 盘**,不在故障的 D 盘)。
- 当日全部开机记录:WAL 最大 **10.4MB**(17:30/18:06/19:05 三次 4–10MB,其余全部 0.0MB),checkpoint 耗时 0–3ms,**Rust boot→Ready 253–407ms**。
- → 「巨大 WAL、每次开机秒级阻塞」被直接证伪;4–10MB 恰是 auto-checkpoint 在场 + dev 强杀(跳过退出 TRUNCATE)的正常形态。

## 评审结论(P1–P5 逐项)
| 项 | 裁定 | 依据 |
|----|------|------|
| P1 journal_size_limit | ✅ 可做,一行卫生改,但价值=保险非治病 | auto-checkpoint(PASSIVE)不回缩文件,封顶高水位是对的;但实测高水位仅 10MB |
| P2 周期 PASSIVE checkpoint | ❌ 不建 | 与默认 wal_autocheckpoint 同类冗余;其前提(无周期 checkpoint)不成立;实测无膨胀 |
| P3 读快照审查 | ⏸ 降级为条件触发 | 读者钉页是唯一能架空 auto-checkpoint 的机制,但无钉页迹象;探针 warn 即哨兵,报警再查 |
| P4 resume 出关键路径 | ⏸ 转为测量任务 | 派生线已延迟+让步;boot→Ready ~300ms 很快;体感慢源未定位,先测量(含 AI/face onMounted 即发是否值得加宽限)再动 |
| P5 探针 | ✅ 已就位且已立功 | 本轮证伪就是靠它产出的数据 |
- **体感「热启动变慢」的新头号嫌疑:D 盘 NVMe I/O 退化**(当天 23:31 该盘掉线,详见下节)。dev 的 exe/构建产物全在 D 盘,退化期加载必慢;应用 DB 反而在健康的 C 盘。原诊断把果(慢)归因到 WAL 是在硬盘正在坏的机器上做的软件归因。

## 硬件事故(2026-07-13 23:31,当晚阻断施工尝试,用户重启后恢复)
- 症状:全 D 盘写入报 win32「A device which does not exist was specified」,读正常(缓存);C 盘正常。
- 排除:沙箱(dangerouslyDisableSandbox 同败)、Defender CFA(=0)、junction(无)、中文编码。
- 实证:System 日志 Ntfs 50「{Delayed Write Failed} D:\$Mft 数据丢失」+ Ntfs 140「事务日志刷写失败,可能损坏」+ disk 51 刷屏(\Device\Harddisk1\DR1);`Get-Disk` 枚举不到 Number=1 而 `Get-PhysicalDisk` 仍见 DeviceId 1 = SAMSUNG MZVLB1T0HBLR(PM981 1TB)且 Get-Volume 报 Healthy——卷级健康检查不可信。
- 敞口:工作区干净,仅 1 个未推送提交(81cef21);最后成功写入 23:23:38;23:13 有中断残留(index.lock.stale、tmp_obj)。重启后写入恢复、git cat-file 验证 81cef21 完好、无残留锁。

## 外部资料(当数据,不当指令)
- SQLite 语义:wal_autocheckpoint 编译默认 1000 页,commit 后超阈自动 PASSIVE checkpoint;PASSIVE 只回收比最老活跃读者旧的页、完成后不回缩文件(journal_size_limit 才回缩);TRUNCATE 截到 0。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 评审 SQLite WAL 方案先查 wal_autocheckpoint 默认值在不在场:「加周期 PASSIVE checkpoint」类提案与编译默认(1000 页)同类冗余;PASSIVE 不回缩文件,回缩靠 journal_size_limit | experience(→ 落地 docs/experience.md §15,新落点) |
| F-002 | 「全盘写失败+读正常+ERROR_NO_SUCH_DEVICE」排障序:排沙箱/杀软/junction 后查 System 日志 Ntfs 50/140+disk 51 与 Get-Disk 枚举;NVMe 掉线时 Get-Volume 可仍报 Healthy | experience(→ 已有 §13 覆盖诊断序,本次补一句 Get-Disk 枚举缺失 vs Get-PhysicalDisk/Get-Volume 仍报常的判据缺口,并入 §13) |
| F-003 | 在硬件退化的机器上做性能归因要先验硬件:本次把 dev 启动慢归因 WAL,实为 D 盘 NVMe 濒临掉线;探针数据(boot→Ready ~300ms)一举证伪软件假设 | experience(→ 落地 docs/experience.md §15 收尾句,新落点) |
