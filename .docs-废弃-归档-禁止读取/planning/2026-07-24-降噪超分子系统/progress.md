---
status: active
type: working-memory
line: 降噪超分子系统
created: 2026-07-24
---

# 进度日志:降噪超分子系统

<!-- 验证行怎么填(F-005):门禁末行须在**全部内容落盘后**才跑得出来——顺序是
     先写占位 → 跑门 → 回填真实末行(改动了再复跑)。别倒过来抄一行旧输出充数。 -->

## 前情(接续先读这段,≤10 行;旧会话细节在 progress-archive.md)
- 当前:设计定案完成,阶段 5(收口本批)已提交;待用户裁决 J-1..J-8(design.md J 节)与批准施工,本线不再自动推进。
- 未解错误:无。
- 关键指针:design.md(方案定案稿)/ scratchpad/ai-pipeline-map.md / scratchpad/plugin-store-map.md / scratchpad/industry-research.md。

## 回顾(收口时填;置于会话段之前——文件尾留给最新会话段,新段追加到末尾)
- 亮点:<什么做法值得复用>
- 教训:<什么坑值得预警>
- 意外:<什么假设被现实推翻>

## 会话:2026-07-24
- 做了:阶段 1–4 完成(仓内摸底×2 + 联网调研 + 架构设计 + 主线裁决定案,产出三份 scratchpad + design.md A–J 十节);阶段 5 收口本批(findings 蒸馏 F-044..F-049、task_plan 折叠、progress 回写、显式 pathspec 提交)。
- 委派账单:general-purpose(haiku)×2@13r/20r(1b 达预算 20/20)/ researcher(sonnet)×1@≈9r / architect(inherit)×1@≈6r / phase-closer(sonnet)×1(本批)/ 主线直做 ≈4 次调用(Bash×1 建三件套 / Read×1 / Write×2 task_plan+design)。
- 验证:`npx worklog-kit doctor` exit 0(仅无关预存告警);`npx worklog-kit check` 首轮 exit 1(4 处本线红=design.md+3 份 scratchpad 缺 `docs/lines/降噪超分子系统.md` 线实体),主线续批授权后已建线实体文件并复跑清零(见下)。本批 commit:docs(planning): 降噪/超分子系统三件套+方案定案。
- 遗留:阶段 5 之后无遗留阶段;⏸ 用户裁决 J-1..J-8 + 批准施工。

## 会话:2026-07-24(续批,主线授权扩一处目录)
- 做了:建 `docs/lines/降噪超分子系统.md` 线实体(照视频播放器重构线式样);task_plan.md/progress.md 内 stale commit hash 引用改为提交标题引用;amend 进同一提交。
- 验证:`npx worklog-kit check` 复跑,本线 4 红清零(RAW 线既有 1 红为基线红,与本线无关未动)。
- 遗留:同上。

## 会话:2026-07-24(P0 施工)
- 用户裁决 J-1..J-8 定案(J-2→模型托管改用户自有 HuggingFace 仓,仓名待补)。排期门核实:RAW 线三文件(exotic-catalog.json / catalog.rs / coordinator.rs)已提交,#5/#6/#8 进场条件满足。
- spike:A(venv+RealESRGAN 导出对拍绿,diff≤3.8e-06/PSNR≥66dB)、B(SCUNet/DRUNet 绿,DRUNet 须 bias=False;RealESRGAN fp16 IO 重转 keep_io_types=True)、C(FBCNN 双输入绿,QF 契约=[1,1] fp32 值大去伪影强)、D(DirectML 实测在途)。5 模型 CPU golden 全达标。
- 批 1(协议三臂+profile,复核 1 警告修:ModelDescriptor.model_id)、批 2(tiling/chain,深审 0 严重 4 警告 2 建议全修,增量核验过)、批 3(enhance-worker crate 落地,models_root 补校验在途)、批 4(host 面在途)。
- 教训入错误账表:协议 crate 加 RequestBody 变体属共享契约面,施工验证漏消费侧 worker check 致 workspace 编译红(psd/ai-worker match 非穷尽),已补拒绝臂并在后续批任务卡钉 workspace check。
- 本段 commit:feat(enhance): ai-core 推理核心——enhance_profile 注册表 + tiling/chain(P0 批1/2)。
- spike-D 完成:RTX 3080 Ti 10/10 组合零崩零 CPU fallback;9/10 PSNR 53.9–130.8dB;GPU 加速 24.8–171.3×;唯一红 scunet-fp16 DML 37.48dB(<40 门,DML 特有 −11.3dB)。硬门判过,短名单无回炉。
- 主线三裁决:①SCUNet GPU 档钉 fp32(110.55dB 达标、41.3× 加速;VRAM delta ≈8.4GiB,8GB 卡 OOM 走 ResourceLimit 转 CPU 建议,低 VRAM 降档 P1);②超时终钉 ENHANCE_SESSION_INIT 90s / ENHANCE_SILENCE 300s(bench:session ≤3.4s、CPU 最慢 tile 36.2s);③J-6 准入终钉草案值(降噪/去伪影 ≤100MP、4x 超分输入 ≤16MP)。
- enhance-worker 深审结论:路径安全(models_root canonicalize 前缀校验+`..`/越界拒绝)/协议循环/错误映射全过,叶级 symlink 按 OCR/CLIP 先例接受(主线裁决);会话槽幂等测(session_id 类型对齐 u64)补齐,22 测绿。协议面两 commit:feat(enhance): exotic 协议 Enhance 三臂 + 消费面接线(P0 批1/批3 协议面)、feat(enhance): enhance-worker 独立进程 worker——会话校验/白名单/tile 进度(P0 批3)。
- 验证:`cargo test -p enhance-worker` exit 0(22 passed);`cargo test -p exotic-protocol` exit 0(34 passed)。

## 会话:2026-07-24(P0 批4/4.5/5/5.5 收口)
- 批 4(host 面,27r 落地):exotic-enhance offering(builtin/paid)+ Capability::Enhance;EnhanceService 内存队列 + GPU 双取 D2/claim → EXIF 注入 → rename → ingest;准入门(100MP/16MP 终钉,RAW 同步拦);per-tile 进度透传;preview 单 tile 真实现;settings 4 键;scunet fp16_safe=false(spike-D 裁决)。reviewer(opus)深审 0 严重。
- 批 4.5(深审九项修正):session 断裂重建、out_tmp 清扫、RAW 同步拦收口到 catalog 单源判定、run_request 进度透传改为零行为委托(4.5 前后 ai::/exotic::worker 终态回归绿,证行为不变)。两轮增量核验通过。
- 批 5(前端全链,11 文件):types/enhance.ts、enhanceStore(订阅计数守卫)、EnhanceDialog(任务勾选/模型档/σ-QF 滑杆/预览容错/门控引导)、BeforeAfterSlider、EnhanceSettingsSection、ContentViewer 入口、i18n 两 locale。复核两存疑:①终态 toast 是否会滞后于队列面板关闭 ②队列面板缺失(design F 节队列进度面板漏列入批 5 任务卡,复核网兜住)。
- 批 5.5(存疑收口):EnhanceQueuePanel(tiles 进度)新增;enhanceStore 终态 toast 改为不滞后补报(即时到达即出,不等面板打开);两处小修。
- 两 commit 落盘:`feat(enhance): host 全链——catalog/门控/EnhanceService/七命令 + 深审九项修正(P0 批4/4.5)`、`feat(enhance): 前端全链——EnhanceDialog/设置分节/队列面板/查看器入口(P0 批5/5.5)`。
- 终态回归(收口本会话实测):`cargo test -p scrollery --lib -- ai::` exit 0(44 passed);`cargo test -p scrollery --lib -- exotic::worker` exit 0(9 passed);`cargo test -p scrollery --lib -- enhance` exit 0(18 passed);`cargo clippy -p scrollery --lib -- -D warnings` exit 0(0 警告);`npx vue-tsc --noEmit` exit 0(0 错);`npx vitest run src/stores/enhanceStore.spec.ts src/i18n/localeIntegrity.spec.ts` exit 0(15 passed)。
- 教训入错误账:批 5 任务卡当时漏列 design F 节「队列进度面板」一项,靠复核网兜住而非任务卡先行覆盖;后续批卡编写须对照 design 节级清单逐项过,不能只凭记忆摘录。
- 遗留:批 6(⏸ 用户 HF 仓名未定,模型托管资产上传阻塞)+ ⏸ GUI 真机验收清单(见下节)。

## 遗留:GUI 真机手测清单(不自动执行)
- 商店卡片显示与激活流
- 模型下载进度
- Dialog 全流程(勾选 → 滑杆 → 提交)
- 队列面板进度与终态 toast
- 预览 before-after(需模型就位)
- RAW 拒绝引导
- 未授权引导
