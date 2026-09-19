---
status: snapshot
type: working-memory
line: AI与人脸流水线根治
created: 2026-07-22
---

# 任务计划:AI与人脸流水线根治

## 目标
根治「点开始后 AI/face 不跑」全因果链:修复派生 pipeline consumer 挂死(video_cover 批),恢复 ai-worker.exe 构建,补齐可观测性与毒任务防线,使 AI/face 流水线可跑且挂死类故障可见可自愈。

## 背景(诊断会话结论,2026-07-22)
- 直接机制:派生 pipeline `run_pipeline_blocking`(derive/pipeline.rs)consumer 挂死不返回,derivation RunTokenSlot 恒 running;AI/face producer 在 `ai_yield_blockers()`(state.rs:780)无限让步(debug 级日志,零可见)。
- 挂死批:DB status=1 恰 125 = video_cover 123 + doc_thumb 1 + audio_cover 1;每次重启孤儿复位再领再卡。
- 时间定界:今晨 07:26–07:28(昨晚 build)video_cover 批量完成 2162 个;下午重编译(含 b216dcc 配置外置 + eef2412 watcher spawn)首跑即挂。**新 build 回归嫌疑 > 毒文件嫌疑**。
- 挂点候选:consumer 节流循环 / `background_heavy_limiter.acquire` / video_cover 解码调用(07-17 DXVA 线未验收)/ **config 锁与 derive 死锁(b216dcc/eef2412 新增嫌疑)**。
- 第二层:仓库迁 C:\workspace 后 target 无 ai-worker.exe(tauri dev 不构建 sibling bin);psd-worker 两 session bad_signature。
- 现场已失:scrollery.exe 已退出;procdump/cdb 未装。挂死稳定复现,可重造现场。

## 当前阶段
阶段 6:全量门禁+真机重跑(phase-closer 执行中);0-5 全部完成

## 阶段依赖 DAG
```
阶段0 取证(0a静态 ∥ 0b运行时备选) ──> 阶段1 修挂死 ──┐
阶段2 ai-worker构建+psd签名(独立) ────────────────────┼──> 阶段6 全量门禁+收尾
阶段3 可观测性三修(独立) ────────────────────────────┤
阶段4 毒任务防线(独立,设计参考阶段0结论) ──────────┤
阶段5 face_enabled 判定小修(独立) ───────────────────┘
```
0a/2/3/5 无互依赖,可并发;阶段1 依赖阶段0 结论;阶段4 施工可先行、参数待0定。

## 阶段

### 阶段 0:挂点取证 — 完成
- 根因定案:MF 同步 `IMFSourceReader::ReadSample` 死等内部事件(mkv 触发 MF_E_UNSUPPORTED_BYTESTREAM_TYPE 错误路径丢事件),死等 reader 僵死 MF 进程级工作队列扩散到后续全部 reader(7 线程实锚);scope 永不 join → token 恒 running → AI 让步;face 另被全局 rayon 池占满饿死。全证据链+五假说证伪过程见 findings 阶段0。
- **状态:** complete

### 阶段 1:修派生挂死(MF 读帧超时化)— 完成(e670369)
- 异步回调+30s deadline 护栏+毒化快速失败+泄漏隔离(含硬解槽泄漏 warn);opus 深审 0严重0警告(2 存疑裁决入 findings);D-003 双钉零触碰、rotation 测试绿;已知边界=seek 不设护栏(归 F-029)。
- **状态:** complete(实机 --rotation-check 与毒批清算归阶段6)

### 阶段 2:worker 构建恢复 — 完成(f3939a5;真机终验归阶段6)
- ai-worker 构建落位+spawn 验证;psd bad_signature=迁移后旧 exe 占位 keyset、18:50 重编译自愈;beforeDevCommand 前置 worker 构建。
- **状态:** complete

### 阶段 3:可观测性 — 完成(874f511)
- 让步日志变化才 info、status 扩 waiting_on+前端等待原因显示(i18n 双语)、派生 300s 看门狗;opus 复核 0 严重,interaction 键/分隔符两修+fixture 补齐已落。
- **状态:** complete

### 阶段 4:毒任务防线 — 完成(acdca33)
- V22 orphan_count:第 3 次孤儿当轮转 error;写结果归零;显式清缓存重生成归零给新预算(复核裁定);特征测试 2 项,derivations 6/6+migration 11/11 绿;opus 复核 0 严重 0 警告。
- **状态:** complete

### 阶段 5:face_enabled 判定 — **no-fix 结案**
- 亲见 runtime_config.rs:100-110:config.toml Bool 类型恒渲染 "true"/"false"(schema 契约),migrate.rs 已归一化旧 "0"/"1",load 层对非法值发 warning 并沿用默认——`== Some("false")` 契约内正确,非缺陷。诊断会话顺手发现经核实撤销。
- **状态:** complete(no-fix)

### 阶段 6:全量门禁 + 收尾
- [ ] cargo test/clippy 全量 + vitest/vue-tsc(涉前端时)
- [ ] 真机复跑:派生完成、AI/face 实际出活
- **状态:** pending

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 取证双轨:静态优先,运行时 dump 备选 | 现场已失且调试工具未装;新 build 回归嫌疑强,静态 diff 交叉性价比高(注:静态五假说全倒,最终靠栈定案——复盘见 findings) | |
| 修挂死前不改 config.toml 禁 video_cover | 保留稳定复现路径供取证;快恢复与根治冲突时根治优先(用户已点根治) | |
| 一期修法=MF 异步回调+超时+COM 泄漏隔离;不做 video-worker 进程隔离 | 超时化止损面已够(毒 mkv 一次超时后 status=3 永不重试;本库 mkv id 最大排队尾,mp4 在其前全清);进程隔离是彻底解但工程量大,记 F 候选二期 | D-401 |
| 不做 mkv 容器级预检挡板 | 部分 mkv(h264)可能正常;超时修后毒文件只损一次 30s,挡板会误伤良性 mkv | D-402 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
