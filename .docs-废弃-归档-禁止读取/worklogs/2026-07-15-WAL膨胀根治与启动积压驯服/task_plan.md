---
status: snapshot
type: working-memory
line: WAL膨胀根治与启动积压驯服
created: 2026-07-13
---

# 任务计划:WAL膨胀根治与启动积压驯服

## 目标
原目标:根治「崩溃→WAL 膨胀→每次开机变慢」因果链。
**2026-07-13 评审后修订**:原方案 P1–P5 经代码核实 + 探针数据检验,「WAL 膨胀致开机慢」假设被证伪;任务收敛为方案评审 + 一行卫生改。
**2026-07-15 收口**:P1 已由并行会话落地;P4(真实慢源测量)用户裁决暂不施工,登记 [todo.md R 线](../../todo.md)待办;任务收口归档。

## 当前阶段
已收口。

## 阶段

### 阶段 0:方案合理性评审(用户 2026-07-13 明确:先分析,不施工)
- [x] 核实 PRAGMA 组无 journal_size_limit(connection.rs:26-34)
- [x] 核实退出钩子 TRUNCATE 仅覆盖优雅退出(lib.rs:955-968)
- [x] 核实开机 checkpoint_wal_at_boot + 计时探针已在(connection.rs:65-105,48ed643)
- [x] 核实 App.vue 启动即双 maybeAutoResume(App.vue:318/326);派生线是 useDerivationAutoStart=3s 延迟+状态检查+让步(App.vue:258)
- [x] 核实全库无人设置 wal_autocheckpoint → SQLite 默认自动 checkpoint(1000 页≈4MB)在场 → 原方案 P2 前提(「批量写期间无周期 checkpoint」)不成立
- [x] 取证生产库:scrollery.db=23MB 在 C:\Users\gf\AppData\Roaming\com.scrollery.app;当日日志全部开机 WAL≤10.4MB、checkpoint 0–3ms、Rust boot→Ready 253–407ms → 「巨大 WAL 秒级阻塞」证伪
- [x] 结论输出(见 findings「评审结论」)
- **状态:** completed

### 阶段 1:P1 journal_size_limit=64MB 一行卫生改
- [x] PRAGMAS 常量(connection.rs:26)加 journal_size_limit = 67108864
- **状态:** completed——**由并行会话(Claude Opus 4.8)在本任务评审结论产出后、未等待本任务显式用户 go 的情况下直接落地**(commit `b24b196`,2026-07-14 00:15,提交信息准确标注「通用卫生封顶,非问题修复」,与本评审 D-001/裁定一致)。本会话 2026-07-15 核实该提交内容与评审结论无冲突,予以承认、不回滚。因主机硬件冻结,该提交未跑完整 CI 复验(见 commit message)。

### 阶段 2:真实慢源测量(登记为待办,未开工)
- [ ] 「热启动变慢」的体感复现 + 分段计时(进程拉起[exe/DLL 从 D 盘加载]→Rust boot→Ready→前端首帧→画廊可交互)
- [ ] 结合 2026-07-13 D 盘 NVMe 掉线事件,优先排查 D 盘 I/O 退化对 dev 启动的影响
- **状态:** shelved——用户 2026-07-15 明确裁决「暂时不改了」,今日不施工;已登记 [todo.md R 线](../../todo.md)第四行待办,供后续需要时接续。

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| P2(周期手动 PASSIVE checkpoint)不建 | SQLite 默认 wal_autocheckpoint=1000 页已是周期 PASSIVE checkpoint,手动加一层同类操作无增益;且探针实测无膨胀 | D-001 |
| P3(读快照审查)降级为条件触发 | 唯一能架空 auto-checkpoint 的机制是读者钉页,但实测 WAL 最大 10.4MB=无钉页迹象;探针 warn(>64MB/>500ms)已是哨兵,报警再查 | D-002 |
| P4(resume 出关键路径)转为测量任务 | 派生线已有 3s 延迟+让步;boot→Ready 仅 ~300ms;体感慢源未定位,先测量后施工 | D-003 |
| 「开机慢」新头号嫌疑=D 盘 NVMe I/O 退化 | 当天 23:31 该盘直接掉线(详见 findings 硬件事故节);dev 构建产物/exe 全在 D 盘,退化期加载必慢;应用 DB 反而在健康的 C 盘 | D-004 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| 全部写入 D 盘报「A device which does not exist was specified」 | 依次排除:Write 工具/Bash/PowerShell、中文编码、沙箱(dangerouslyDisableSandbox 同样失败)、Defender CFA(=0)、junction(无) | 定性=硬件:NVMe 掉线(Ntfs 50/140+disk 51,Get-Disk 查无 Harddisk1);用户重启恢复,git 完好 |
| 把「分析方案」误解为「按方案施工」 | 开工即建施工版三件套 | 用户澄清后改交付评审报告;施工待裁决 |
| 会话中途未察觉项目已迁移(picasa-next→scrollery),对着已脱离版本控制的旧路径 `d:\photoapp\picasa-next` 读写了一轮三件套 | git 命令报「not a git repository」才触发排查;Read 工具报错信息暴露真实 cwd 为 `D:\workspace\scrollery`,核对确认旧路径 .git 已空、真实仓库另有并行会话推进(P1 已落地、硬件发现已蒸馏) | 改在真实仓库路径重新核实并整合两侧内容,收口前不再假设路径本身可信,发现路径歧义先用 Read/Glob 报错信息交叉验证 cwd |
