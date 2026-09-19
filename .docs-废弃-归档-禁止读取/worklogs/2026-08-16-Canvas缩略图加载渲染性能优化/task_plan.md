---
status: 快照
type: 工作记忆
line: Canvas缩略图加载渲染性能优化
created: 2026-08-16
---

# 任务计划:Canvas缩略图加载渲染性能优化

## 目标
按《[2026-08-16-Canvas画廊缩略图加载渲染性能审查](../../reviews/2026-08-16-Canvas画廊缩略图加载渲染性能审查.md)》修订版 §5 的实施顺序,以「先测量、后实验、无数据不重构」的纪律,消解 4K 全屏密集格下的 480px 冷区症状,并补上已确认的生成链路恢复缺口。

## 当前阶段
阶段 5:收口(阶段 0/1/2/3/4 全部完成;4a/4b/4c 均为数据裁决的 no-op 或不采纳,详见各阶段节)

## 阶段

### 阶段 0:测量脚手架与能力盘点(2026-08-16 完成)
- [x] 盘点:`canvas-wave-bench.mjs` 只有单场景冷滚(恒速/burst 剖面已有);`performanceRecorder` gauge last-write-wins 无时序;无进程内存采样、无落盘。**并发现 harness 两处既有致命缺陷**(见 findings F-004):缺 `__TAURI_INTERNALS__` 桩导致 `new Channel()` 构造抛错(批量生成链整链死亡)与 `convertFileSrc` 在 drawCell 热路径同步抛错(status 1/3 场景逐帧 rAF 异常、gauge 全空)——当前代码上旧 bench 的 cold 场景同样炸,非本线引入
- [x] bench 补五场景 `--scenario=cold|warm|ungenned|offline|missing|selectanim`:warm=同剖面预热趟(不入样本)回顶再录;ungenned=`&thumbStatus=0`+fixture 模拟批量生成(30ms/项经 Channel 逐项回填,先送达后 resolve 防 incomplete 重试);offline/missing=`&avail=&availRatio=`(默认 0.8);selectanim=Ctrl+点格(坐标从 canvas 视口矩形反推,固定坐标会点进侧栏)→ Ctrl+A 全选→滚动,DOM 探针验证选中态
- [x] 时序采样:录制期 250ms 轮询 `__scrolleryBench.sampleGauges()`(recorder 新增 `currentCounters()` 浅拷贝,不触样本缓冲)+ `performance.memory`;gauge 语义未动
- [x] 进程内存:Node 侧 500ms PowerShell `Get-Process` 采工作集(默认采 headless Chrome 子进程,`--pid` 可采外部进程供阶段 1 真机用)
- [x] headless 冒烟:六场景 × constant + cold × burst 全通,数值分化符合预期(warm 冷格帧 ≈ cold 的 1/5,ungenned 最差,selectanim 选中态 true);`--out` 结构化 JSON 含 meta/session/timeseries/procMem/pageExceptions;CDP 捕获页面未捕获异常(诊断利器)
- **状态:** done

### 阶段 1:真机基线测量(本机即目标机;用户拍板用 dev 构建,2026-08-16 完成)
- [x] 方案决策(用户拍板):真机 = `npm run tauri dev` + `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223`,WebView2 CDP 直驱;dev 前端桥可用
- [x] 图库盘点:真实库 65,594 项(已生成 99.8%,未生成仅 114 → 生成段无场景);`grid_row_height=64`(用户现行);thumb_size 512;缓存 3.6GB/12 万文件
- [x] rig:`scripts/bench/canvas-realapp-bench.mjs`(attach WebView2 + 同源滚动驱动 + 时序/内存/异常采样);排障:app 主窗口保存在屏外 (-21333,-21333)、158×26 → 布局「width<100」放弃 → Win32 MoveWindow 移正后一切正常(F-007,窗口状态损坏是独立产品问题)
- [x] 基线 10 run(冷 n=4/暖 n=2/全选态 n=2/快滚 n=1/暖区复测 n=1,6s×1.5px/ms,120Hz):数据与结论见 findings「阶段 1」节,原始 JSON 存 `baseline-realapp/`
- [x] 分段定位:生成段本库不可测;加载段冷波 jank 1.7–59% 区域差异大且冷格量≠掉帧;绘制段常态非瓶颈,**全选态滚动 jank 44–60% 为最重场景**
- [x] 门控裁决:阶段 2 开工(输入像素量获间接支持);4b 维持门控;**4c 选中态绘制升格为真机已命中**;4a 维持既有反证门控
- **状态:** done

### 阶段 2:多档缩略图源 120/240/480 A/B(2026-08-16 完成;实际档位体系为 [64,128,256,512,1024],实验落 128/512 对照)
- [x] 盘点:档位路径 `{tier}/{xx}/{hex}.webp` 确定,cache_dir 已有分层目录;视口批量对未生成项已按行高档位生成;缺口 = 已生成项服务固定回 DB 旧档
- [x] 2a 服务选档(19cad9b):`thumbnail/serve.rs`(pick_serving_tier 纯函数 + ThumbServe probe/rewrite,3 单测)+ hydrate_item/hydrate_rows/三出口接线 + LayoutItem.cache_key + DPR 经 compute 上报(AppState 原子);生成档位×DPR 对齐(前端 targetSize 乘 dpr——修「DPR1.5 生成 64 档永不满足服务需求」脱节)
- [x] A/B(数据 `ab-stage2/`,裁决见 findings「阶段 2」):同区前后 P50 37.5→29.75ms(−21%,混合密度 30%);页内双档微基准:热缓存单件两档均 2-3ms → **冷装载时延主因是并发排队而非输入像素**;全库磁盘 128 档中位 2.5KB(512 档 24.8KB),回填估算 +145MB(+9%)
- [x] 裁决:服务选档+生成对齐保留;全库存量回填维持不做(渐进收敛);4c 升为下一优先;4b 扩槽升格可测候选
- [x] 排障记录:app 视图排序与 SQL (ts,id) 序不一致 → 选区散布(以 y 轴扫描定位主簇重做);ORDER BY id 小样本选样偏置教训
- **状态:** done

### 阶段 3:生成链路恢复缺口修复(正确性,无测量依赖——用户拍板提前,2026-08-16 完成)
- [x] characterization 先行:以现有 `canvasThumbState` / `useRequestQueue` spec 锁当前行为,先补缺失用例(现有 20 用例已锁全部消息字符串与槽位语义,直接在其上扩展)
- [x] 重试落点裁决:**队列内有界退避重试**(`useRequestQueue` 拒绝原因类型化:cancelled 不重试;stalled/incomplete 按 0.5s→1s→2s 最多 3 次重试)。`canvasThumbState` 零改动——canvas 的 `requestThumbOnce` 只见一次逻辑请求,收敛发生在队列内部,同签名不再有静默失败窗口;`MediaGrid.vue` 亦零改动。原设想的「canvasThumbState 显式重试面」不再需要
- [x] cancel 掐断退避等待期(等待期与新建 slot 可并存,先掐链再走既有 slot 路径)
- [x] 单测:改 4 个既有用例适配重试时序 + 新增 3 个重试语义用例(收敛/掐断/挂回在途 slot);全量 vitest 1535 用例过,vue-tsc + ESLint 干净;CI 前端 job 已含 `npm test`
- **状态:** done

### 阶段 4:门控实验组(仅按阶段 1 命中数据逐项开工;未命中项记 no-op 不做)
- [x] 4b 64/128 并发 A/B:冷区 6 区域配对(每 run 重启)——**裁决不采纳**:coldFrames 被区域差异主导(132↔24,699)无法配对,判据「冷格改善」未证明;无回退信号(draw/内存/fps 两臂无差);维持 64。重访前提 = 同区域同实例内可切换槽位
- [x] 4a 近桶源跳过第二次 Bitmap 转换:**维持既有反证门控,no-op**(阶段 2 已证热缓存单件 2-3ms、瓶颈在排队而非解码;无新数据支持重试)
- [x] 4c 选中态绘制:**裁决 no-op**。E 系列开关(降级实例)mask+ring≈0.9ms/clip≈0.7ms/手柄≈0 误导性地支持批处理;真机重启制交错 A/B 证伪:batched 43fps vs legacy 118-120fps(draw span 变快但帧光栅化恶化),2×2 归因 = 联合 clip 灾难级(6.7fps/150ms 帧)+ 巨路径 mask/ring 回退(40→59fps);**legacy 逐格绘制在健康实例贴满 vsync,阶段 1 的 44-60% jank 基线是降级实例伪影**。批处理代码(selBatch/联合 clip/selectedCellGeom)全部回退,零残留
- [x] 每实验独立小批次提交,保留回滚开关(参数/分支),回滚线写进提交说明 → 4c 无提交(回退干净);教训入 findings F-011(实例方差污染绝对值,须同实例/重启制交错 A/B)
- **状态:** done(4a 维持反证门控 no-op;4b 不采纳维持 64;4c 批处理否决回退)

### 阶段 5:收口
- [ ] 实测结果回写审查报告(补「实测结果」节或在文首标注后续验证状态)
- [ ] `docs/todo.md` 对应项状态回写;蒸馏候选处置;目录迁 `docs/worklogs/`
- **状态:** pending

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 施工顺序沿用修订版报告 §5;阶段 3 为正确性修复且无测量依赖,拟作为穿插批次提前做 | 不重排已裁决的优先级;恢复缺口不依赖基线数据,先行可减少等真机测量的空窗 | |
| 用户拍板(2026-08-16):阶段 3 提前做;本机即目标真机 | 阶段 1 真机基线可在本机执行发布构建完成 | |
| 重试落点 = `useRequestQueue` 队列内部(类型化拒绝原因 + 有界退避),`canvasThumbState`/`MediaGrid` 零改动 | 调用方 catch 只留占位、canvas 同签名只上抛一次,队列内收敛是改动面最小且双消费方(MediaGrid/HGalleryLabView)免费受益的落点;后端对在途项 single-flight,停滞重试不会重复生成 | D-001 |
| harness 最小 `__TAURI_INTERNALS__` 桩装在 main.ts 的 isTauri 判定之后:transformCallback 回递增 id 不注册回调、convertFileSrc 直通、invoke 拒绝 | 适配层(appEvents/appWindow)全走 isUiHarness 守卫不受影响;isTauri 判定先于装桩防窗口分流误入;invokeIpc 的 harness 分支绕过 invoke,误入者(如 event.listen)得可读拒绝 | D-002 |
| 真机基线用 dev 构建(tauri dev + WebView2 调试端口 CDP 直驱)而非发布构建 | 用户拍板;asset 协议/后端/渲染全真且 dev 桥可用,采集全自动化;代价(发布态差异)在结论中标注 | D-003 |
| 阶段门控裁决(基于阶段 1 真机数据):阶段 2 开工;4c 选中态绘制升格为已命中;4b 维持门控;4a 维持既有反证 | 见 findings「阶段 1」分段定位:全选态滚动 jank 44–60%(4c 真机命中);r4/r5 反证削弱槽位等待链(4b);生成段本库无场景 | D-004 |
| 阶段 2 裁决(基于 A/B):服务选档+生成×DPR 对齐上线;全库回填不做(新生成渐进收敛);4b 扩槽因「排队主导」微基准升格可测候选 | 单件两档均 2-3ms(热缓存),冷装载时延是并发排队;多档并发收益中等(−21% P50);磁盘 +9% | D-005 |
| gauge last-write-wins 语义不动,时序化只在测量侧加采样 | 生产行为零改动,测量能力按需补 | |
| 阶段 2 的存量缩略图回填默认不做 | todo.md:439 明示低优先级;避免长尾 IO 批次 | |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
