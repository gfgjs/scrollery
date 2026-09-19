---
status: 快照
type: 工作记忆
line: 图片编辑功能升级
created: 2026-07-19
---

# 进度日志:图片编辑开源包混合升级施工

## 会话:2026-07-20(续,阶段 7)
- 做了: 完成阶段 7 整体验证与文档回写(全部七阶段收官)。CI 对应门禁全套本地实跑(Windows fmt/check/clippy/test、经 WSL 复核 Linux check/test、渠道依赖树断言、NOTICE 新鲜度、前端 lint/typecheck/test/build/渠道合规扫描、`tauri build --no-bundle` + 开机冒烟)。依赖门:`cargo tree -e features` 确认 imageproc 未反向打开 image 默认格式族,MSRV 达标。NOTICE.md 首次为本线新依赖(imageproc/moxcms/Cropper.js 系列)重新生成,strong-copyleft 0,lcms2 正确因 dev-only 不入账。release 包体差值:临时 `git worktree` 重建 v1 基线(e3020c8)与当前 HEAD 各一份 release 二进制,实测 +2.50 MiB(+6.07%)。D-008 内存基准新增 `full_v2` 探针模式(直调生产 `geometry::apply_geometry`/`adjust::apply_adjust`),在 24/50/100MP 三档测得 v1 基线与 v2 最坏可达场景;发现 100MP+45° 拉直会被既有 `memory_budget` 准入门在解码前拒绝(与既有单测一致),已如实记录为验证证据(非缺陷,不翻案既有裁决)。四平台手测缺口如实记录:项目当前 CI 无 macOS/iOS/Android 覆盖(非本线新增缺口),阶段 7 已到达 Windows 实跑 + Linux 复核 + 开机冒烟的自动化边界,GUI 触摸手势/广色域目视一致性/移动端交叉编译等留待真机验收。文档回写:三件套本文件 + `docs/todo.md` 新增日期条目 + 设计稿 last-verified 戳记与验证附注。
- 验证: 全部证据见 findings.md「阶段 7 实测证据」一节,不重复罗列。
- 遗留: 真机/真实设备 GUI 验收(触摸手势、广色域目视、macOS/iOS/Android)——不可自动化,留给用户;本会话结束前提交并按用户指示 push。

## 会话:2026-07-20
- 做了: 完成阶段 6 E3 真色彩调色。Rust 新增 `editing/adjust.rs`: 三参数公式(亮度 2^(b/100) 乘法、对比度线性系数、饱和度 W3C saturate 矩阵, 序钉死亮度→对比度→饱和度、每步 clamp、alpha 不动), `EditOps.adjust` 走 Option+serde default, 全零经 `effective_adjust` 过滤保 D-106 字节级直通; 有 adjust 时 moxcms f32 分条转 sRGB、位深保持(8→8/16→16/f32→f32)、输出嵌显式 sRGB profile。前端新增 `adjustFormula.ts` 同源公式 + `useAdjustPreview.ts`(canvas rAF 节流, 一帧合并末参数)+ EditOverlay 三滑杆与 canvas/img 互斥切换; useImageEditor 扩三 ref 入 hasChanges/reset/save 载荷。共享黄金向量 `src/fixtures/adjustGolden.json` 12 组(含量化端点与组合序敏感向量)双端消费。
- 验证: cargo test 全库 781 绿(editing 65, 新增 14: 黄金对拍、域校验、中性过滤、untagged 变体/alpha/灰度恒等、tagged sRGB≈untagged 容差 2、16-bit 位深保持、GrayAlpha 升 RGBA16 且 alpha 透传、损坏 profile 稳定码、sRGB profile 往返、条界一致性); vitest 全量 1280 绿(新增 adjustFormula 3、useAdjustPreview 3、useImageEditor 调色 3 含保存载荷契约); vue-tsc、触面 ESLint、rustfmt(本线文件)全净; clippy 仅剩 HEAD 既有 1 警告。
- 遗留: 阶段 7 整体验证与文档回写由用户执行(验证矩阵含 D-008 进程树基准、依赖门、四平台手测); 教训: `cargo fmt -p -- <files>` 会全 crate 重排已回滚, 全角标点 Edit 锚失配改短锚。
- 教训(管道): 本会话 Edit 参数中全角逗号/顿号/分号再次概率性折半角致锚失配, 全角冒号/括号未见折; 新写 doc 内容改用半角标点避险。

## 会话:2026-07-19
- 做了:完成阶段 5 E2。`EditOps.rotateFine` 以 `Option` + serde default 保持 v1 载荷兼容；闭区间 [-45°,45°] 拒绝越界、NaN 与无穷。后端按 D-107 在 flip 后做 imageproc Bicubic 展开旋转并自动裁到保持原比例的最大内接矩形，0° 完全跳过插值；前端加入 0.1° 滑杆、拖动三分网格和同源几何。
- 验证:Rust editing 测试 51 项通过，其中含 v1 payload/零值像素不变、±45° 端点、越界/NaN/∞、16-bit 变体、共享黄金向量与 expand 内存预算。前端 typecheck、ESLint 及 straighten/useImageEditor/Cropper 聚焦测试 7 项通过。
- 遗留:进入阶段 6 E3；需把源 ICC/位深策略贯穿保存链，落地三项调色 Rust/TS 同源公式、预览 rAF 节流及全零字节级短路。
- 做了:完成阶段 1 的三个准入 spike、阶段 3 E0 预览资源链与阶段 4 E1 裁剪交互。后端新增 moxcms ICC→sRGB、orientation 烤入、长边 2048 降采样及版本化二进制预览 IPC；前端改用可回收 blob URL。裁剪采用 Cropper.js 2.1.1 的透明选择层，`useImageEditor` 继续作为唯一状态源。
- 验证:Rust editing 聚焦测试 42 项通过；P0-CM 覆盖 sRGB/Adobe RGB/Display P3、16-bit 灰度与损坏 profile，已测相对 lcms2+BPC 最大误差 2 个码值。前端 preview/cropper 坐标测试 5 项、typecheck 与 ESLint 通过；浏览器真实 DOM 拖动使选择框由约 577×433 变为 487×373，未见新增控制台错误。
- 遗留:进入阶段 5 E2 拉直；需共享最大内接矩形公式，接入 imageproc Bicubic，保证 0° 精确走 v1 路径，并补 90/270°、翻转与拉直组合黄金测试。
- 做了:完成阶段 2。新增 `feature-editing` / `editing-tools-2026` 后端可信常量，授权查询、激活、撤销 IPC 与保存真门；查看器编辑入口保持可见，未授权时复用 `PluginGate` 与可注入激活处理器的既有激活弹窗。授权查询失败前端 fail-closed。
- 验证:`cargo test -p scrollery editing::entitlement --locked` 2/2 通过；前端 `vue-tsc`、ESLint 与授权相关 25 项 Vitest 通过；`git diff --check` 通过。
- 遗留:阶段 3 的预览 IPC 创建时必须在后端入口复用同一 `require_editing_entitlement`；完成 P0-CM 对拍后再定 CMS 生产模块。
- 做了:按用户要求先提交 v1；提交 `e3020c8` 包含 Rust 编辑管线、IPC、Vue 编辑覆盖层、i18n 与测试。随后恢复 v2 阶段 1，完成依赖/MSRV/feature、CMS API、裁剪双候选维护性与审计、exotic 授权复用点盘点，并接入 `imageproc 0.27`、`moxcms 0.8.1` 与仅开发期使用的 `lcms2 6.1.1`。
- 验证:v1 的 `cargo check/test/clippy` 与前端 lint/typecheck/test/build 全部通过；Rust 主库 750 项、前端 1258 项测试通过。v2 依赖接入后 `cargo check -p scrollery` 通过；`cargo tree -p scrollery -e features -i image --locked` 未见 imageproc 打开 `image/default`。
- 遗留:完成 P0-CM 像素级对拍和裁剪实际集成评分；阶段 2 先落地虚拟内建编辑 feature 的前后端 fail-closed 授权门控。
- 做了:读取 `/planning` 技能、`docs/README.md`、v2 设计稿、CI 配置与工作树；建立独立 v2 三件套并拆为七个阶段。
- 验证:`git status --short` 确认 v1 编辑实现及其他在途修改均保持未提交原状；设计稿完整读取共 247 行。
- 遗留:从阶段 1 接续，盘点 v1 编辑链与 exotic 授权链，随后执行三个准入 spike。

## 回顾(收口时填)
- 亮点:双端同源公式 + 黄金向量 fixture(直读法+知识台阶护栏)让「禁 CSS filter」的施工纪律真正落地可验证,而不是只停留在设计稿口号;D-008 从「测探针进程」升级到「直调生产函数测真实链路」后,反而测出既有 100MP+45° 准入门会先一步拒绝的事实,证明测法升级本身有价值。
- 教训:`cargo fmt -p <pkg> -- <文件>` 的 `--` 后面是 rustfmt 参数不是文件过滤器,再次踩过一次(已改用裸 `rustfmt <文件>`);历史提交(e3020c8)重建 release 二进制时先后遇到两个互相矛盾的编译错误,系本机既有硬件不稳定(见 dev-machine-hw-instability 记忆)导致的瞬时 flaky,原样重跑即过,不要在没有先核对源码/锁文件字节级一致前就动手"修"看似的 bug。
- 意外:release 包体差值(+2.50 MiB)与 D-008 内存增幅(CMS 有标签路径仅 +2.7%~+9%)均低于预期,imageproc/moxcms 的工程成本比设计稿工作量估算隐含的风险更小;四平台手测缺口盘点时发现项目当前根本没有 macOS/iOS/Android 的 CI 覆盖(不是本线漏做,是全仓现状),值得记入待办供后续独立立项。
