---
status: snapshot
type: working-memory
line: Canvas快速拖动防闪烁优化
created: 2026-07-17
---

# 进度日志:Canvas快速拖动防闪烁优化

## 会话:2026-07-17
- 做了:读取 planning 规范、文档治理契约与上一轮 Canvas 缩略图优化 worklog；确认工作树开工时干净。
- 做了:将本轮范围收紧为滚动条拖动期间的画面稳定性，明确不以增加 480px 冷区请求并发为默认解法。
- 根因:默认 bucket 引擎远跳时先换愿望段、后单飞异步回填行，`canvasRows` 会短暂为空或不完整；Canvas 原先每次 draw 都先整幅铺底，因此过渡态直接提交白/底色帧，下一帧行到达又显图。
- 做了:新增纯函数判定当前挂载行是否连续覆盖视口；滚动中若已有完整帧但新行覆盖未闭合，则不清 backing store，保留上一完整帧。停滚时强制重绘，避免 IPC 失败时陈旧画面永久残留。
- 验证:`npm.cmd test -- --run src/components/media/mediaGridCanvas.helpers.spec.ts` 通过 1 文件/58 测试；`npm.cmd run typecheck` 通过。
- 验证:DEV harness 5000 项、约 140px 行高、单次 12000px 极速滚动正常落点，Canvas 未回退且控制台无 error/warning；原生滚动条拖动需用户真机复验。
- 验证:`npm.cmd run lint` 与 `npm.cmd run build` 通过；build 仅报告既有 `scanStore` mixed import，入口 578.32/620kB。
- 用户复验:第一版不合格——极速滚轮/滚动条明显顿挫，极速拖动白屏，中低速仍闪；用户明确拒绝冻结，要求快速拉动持续正常显示缩略图。
- 做了:完整撤销帧保持、覆盖判定及其测试，三个 Canvas 相关源码文件恢复到上一提交状态。
- 做了:第二版改供给链。bucket 取行常态单飞，在途段已陈旧时允许最新目标段占第 2 槽；Canvas 可见区快滚持续加载，视口外预取仍关闸；跨一整屏时取消仍可中止的旧 fetch/blob 并释放槽位，解码中任务继续受 64 上限记账。
- 测试:补强 bucket 最新目标立即旁路、连续远跳最多 2 个 IPC、槽释放后只追最新目标，以及 Canvas 在途取消不污染失败态；聚焦 3 文件 110 测试通过。
- 验证:`npm.cmd run typecheck`、`npm.cmd run lint`、`npm.cmd run build` 均通过；build 仅有既有 `scanStore` mixed import 警告。
- 验证:完整 `npm.cmd test -- --run` 通过 88 文件、1163 测试。
- 验证:DEV harness 5000 项，分别在约 64px 密集行和 960px 大行高以 19ms/27ms 拖动逻辑滚动条到远端冷区；落点后 80ms 的 Canvas 均由真实图源完整覆盖，未出现整片白，控制台无新增 error。
- 用户复验:第二版闪烁仍存在，仅少量减轻；供给优化保留，但不足以解决底色/真图切换的感知问题。
- 做了:第三版新增六主题 `--color-bg-canvas-placeholder`；Canvas 整幅清屏和无 ThumbHash 格子统一使用低饱和中灰，DOM 与应用主背景不变。
- 验证:主题契约 + Canvas/bucket 聚焦 4 文件 119 测试通过；`npm.cmd run typecheck`、`npm.cmd run lint` 通过。
- 验证:完整 `npm.cmd test -- --run` 通过 88 文件/1163 测试；`npm.cmd run check:contrast` 全主题硬门槛通过；`npm.cmd run build` 通过，仅有既有 `scanStore` mixed import 警告。
- 遗留:第三版进入真机复验；重点裁决闪烁是否从刺眼白闪降为可接受的柔和灰闪，并按观感继续微调 token 明度。

## 会话:2026-07-17(续,阶段 6)
- 用户新反馈:稍快速滚动(未触发闸门的区间)能看到明显的自上而下真图替换占位图;DOM 模式多渲染一屏慢速无感、快速仍可见;本会话主攻 Canvas。
- 代码审计定位三个候选机制(详见 findings.md 阶段 6 节):①预取只在 requestIdleCallback 分片中执行,而滚动中 currentY watch 每帧取消计划、draw 重建,分片难以稳定抢到帧内空隙;②分片预算按「走过的项」计 64,计划每帧重建后迭代器从暖头重扫,预算被已缓存/在途项耗尽,冷尾永远够不着;③视口 >800px 时 1.25 屏预取像素窗越过挂载窗 ±1000px(次要)。
- 计划:先补探针(visibleColdCells 逐帧规;coldCellFrames 累计冷格·帧积分;prefetchLoadsStarted)+ DEV 基准桥 → headless Chrome+CDP 在 harness 复现并取基线 → 修复(预算按新启动数计 + draw 尾有界同步分片)→ 同脚本 A/B。
- 做了:探针四项 + `window.__scrolleryBench` DEV 桥(import.meta.env.DEV 门控,prod bundle 实测 0 引用)+ harness `&rowHeight=`/`&bucket=` 参数 + scratchpad CDP 基准脚本(burst 速度剖面模拟滚轮连拨)。
- 测量:开闸恒速 1.5/5 px/ms 冷格·帧=0(证伪「rIC 饿死」假设于 headless 域);burst(峰 14/谷 3.15,200px 行)=4911;恒速 12(整程关闸)=14625 → **根因=闸门关闭期视口外预取整体停取**,滞回带把闸门钉住、格子进视口才起载。
- 做了:第四版修复——关闸期预取收缩为仅前向(定档 1.25 屏,1.0 屏实测余 939/1.25 屏余 ~530)+ 后方 0;分片预算按新启动数计(runPrefetchWalk 纯函数抽 helpers,6 新测);draw 尾同步小分片(硬预算 1ms/16 启动)双通道保底。
- 验证:同脚本 A/B burst 4911→510/569/499(三重复,−89%),载入均时 131.7→94.4ms,帧健康两侧一致(120fps/jank 0);开闸域仍 0 冷格。真飞掠(v12)与 60px 极密域受 dev server 每主机 6 连接吞吐钳制(~150 req/s),harness 结论不覆盖(真机 asset 协议无此限)——如实标注。
- 验证:全量 vitest 88 文件/1184 测试、typecheck、lint、build(入口 578.82kB)全绿,均仅本地。
- 遗留:真机复验两项并一轮——①第三版灰底占位观感;②本版稍快速滚动(滚轮连拨/中速拖动)换图波是否消失;极密 60px 域修复效果 harness 不可证,真机顺带观察。
- 用户真机复验(第三轮):标准窗口改善较大;4K+全屏+64px 中低速无感、较快时加载仍明显——用户裁定边际收益不高,记档后期优化,本轮收口。
- 用户真机新报 UI 症状:①占位格糊成连续灰场(D-003 清屏色=格面色,4px 格缝零边界);②中性灰底致真图之间区分度下降。
- 做了:拆双 token 修复——六主题新增 `--color-bg-canvas-gap`(格缝/整幅清屏,比格面暗 ≈6-8% 明度,同色相中性);Canvas 清屏改用 gap 色,格面保持 placeholder 色。格界与图间分隔恢复,差值刻意小不回退防闪收益。
- 验证:主题契约 9 测试(含幽灵/死 token 双向门)、check:contrast 全主题硬门槛、typecheck、lint、全量 vitest 88 文件/1184 测试、build(入口 578.82kB,仅既有 scanStore mixed import 警告)全绿,均仅本地。

## 回顾(收口时填)
- 亮点:①「先测量后动刀」纪律兑现——候选假设(rIC 饿死)被基线直接证伪,避免第三次盲修,真根因(闸门语义)靠探针+速度剖面实证;②可脚本化 A/B(headless+CDP+burst 剖面)让修复效果有 −89% 的数字证据而非观感之争;③症状分层分治:供给链(第二版)、底色(第三版)、闸门语义(第四版)、格界(第五版)各治各的,互不推翻。
- 教训:①滞回闸门对突发型输入(滚轮连拨)会把瞬时越线放大成持续关闸——速度阈值类机制须按输入包络而非峰值标定;②收敛颜色降闪时须列全相邻色对(格面/格缝/真图三方)再动手,只看单一色对会制造新症状(占位糊连、真图失分隔);③dev server 每主机连接数钳制使 harness 在高吞吐域给不出证据——基准评估边界必须如实标注,不可用绿色数字冒充覆盖。
- 意外:①「rIC 分片在滚动帧里饿死」直觉假设在 headless 被证伪(716 片/9s 照常跑);②真机主诉域是 burst(闸门翻动)而非恒速快滚,尽管后者冷格积分高 3 倍——用户感知的是「稍快就糊」的意外感,不是绝对冷格数;③4K+全屏+64px 极密域连供给侧优化也压不住,上限在 IO/解码吞吐,调度层已到边际。
