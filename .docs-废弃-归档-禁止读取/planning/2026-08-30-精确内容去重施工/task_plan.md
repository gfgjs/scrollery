---
status: 施工中
type: 工作记忆
line: 去重功能全局方案
created: 2026-08-30
---

# 任务计划:精确内容去重施工

## 目标
完成 Scrollery 字节级精确重复分析、重复组复核、应用回收站与安全物理清理闭环，并以测试和平台边界证据收口。

## 当前阶段
阶段 5：验收、文档与归档（修复批次验收中）

## 阶段

### 阶段 0：事实复核与边界冻结
- [x] 复核扫描、schema/hash、删除/UI/测试事实
- [x] 冻结各波次子代理写入文件范围
- [x] **状态:** completed

### 阶段 1：P0 + P1 精确索引地基
- [x] 修复扫描 token/seen generation 竞态
- [x] 加入 source revision、纳秒 mtime、dedup sidecar
- [x] 实现流式精确摘要、快速筛选、漂移校验、Live Photo 组合摘要
- [x] **状态:** completed

### 阶段 2：P2 任务与查询
- [x] 实现可停止/续跑分析任务和进度快照/事件
- [x] 实现 duplicate group/member keyset 查询与性能测试
- [x] 完成 IPC 稳定错误契约和前端 store/API 类型
- [x] **状态:** completed

### 阶段 3：P3 复核与应用回收站
- [x] 实现独立 `/duplicates` 虚拟化聚合页
- [x] 实现 keeper、保护标记、冲突与 stale 复核
- [x] 实现 cleanup preflight 与应用回收站软删除
- [x] **状态:** completed

### 阶段 4：P4 物理清理
- [x] 实现 cleanup journal、预检、同卷 quarantine、应用回收站和安全失败保留
- [x] 实现恢复/对账与路径/物理身份安全边界
- [x] 明确 Live Photo、网络卷、移动端和系统回收站能力边界（系统交接 fail closed）
- [x] **状态:** 安全基础 completed；系统回收站交接 deferred

### 阶段 5：验收、文档与归档
- [x] 执行最终 CI 等价检查和主代理复核（最终只读终审代理未返回终稿，已在状态片中记录）
- [x] 回写设计、status、todo 与工作记忆（含最终 Rust/前端数字）
- [x] closeout 并迁移到 worklogs（详见 closeout.md）
- [x] **状态:** completed

## 关键决策
<!-- 候选 ID 在施工中登记，收口时逐项进入 closeout.md。 -->
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 首版只把字节级 exact digest 作为重复与清理证据，现有 change fingerprint 不参与删除决策 | 抽样摘要存在大文件假阳性，且 mtime/复制语义不足 | D-001 |
| 保持 media_items 位置项模型，使用 dedup_index sidecar 动态分组 | 减少迁移面并保留现有关联数据 | D-002 |
| 系统回收站失败时保留 DB 行；Live Photo 物理清理无法保证组件原子性时禁用 | 防止用户元数据与物理文件失配 | D-003 |
| 首版只开放库级分析；非空根范围稳定拒绝 | 根范围查询/失效边界尚未形成可验证契约，拒绝比静默扩大分析安全 | D-004 |
| mtime 变化且 change fingerprint 相同时，抽样 `sha256s:` 切换 source_revision/普通缓存代次并清 exact sidecar；可信完整 `sha256:` 可保留普通缓存；两者都不复用 exact sidecar | 抽样指纹可能漏掉大文件未采样区域，exact 证据必须重新计算 | D-005 |
| 物理清理先写 V29 quarantine_path、同父目录原子隔离并在 writer 锁外恢复；回收站失败不得删 DB | 路径型回收站 API 无法绑定预检句柄，需把已复核目录项与后续路径动作解耦 | D-006 |
| cleanup succeeded 按 item 独立恢复，混合批次不可因失败尾部阻塞成功前缀 | 每一条 succeeded 都是独立的系统回收站证明，整批 gate 会让安全收尾永久悬挂 | D-007 |
| 根路径规范化保留 POSIX `/` 与 Windows `C:/`，但 `C:` 仍保持盘符相对语义 | 丢失根标记会把绝对扫描根存成不同语义的相对路径 | D-008 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| 同大小文件 mtime 变化但抽样指纹相同会复用旧精确摘要 | 仅比较 `content_hash` 基线会留下 stale `dedup_index`，且无法挡住并发旧任务写回 | touch 路径推进 `source_revision`、清理自身和 Live Photo 主项 sidecar，并补 companion 回归测试 |
| 终审发现路径根标记、混合 journal 恢复、回收站对象窗口、mtime 同值 size 变化和前端迟到响应风险 | 初版实现默认了整批 journal 与路径复核足够安全 | 分别补上 V29 quarantine/启动恢复、逐 item DB 收尾、原子 rename 交接、size 参与失效和请求 generation；各有回归测试 |
