---
status: 快照
type: working-memory
line: RAW等格式照片支持
created: 2026-07-24
---

# 进度日志:RAW 等格式照片支持

## 前情(接续先读这段,≤10 行)
- 当前:**阶段 C 全线收官,prod 打包链闭合**;C-2b-prod commit fb5062f(enhance 三变体兜底臂 fix)+ c954865(externalBin 主批);docs 实体 commit 71a0b7f。
- 未解错误:无。
- 关键指针:方案 commit b44459c;P0 施工 commit e28c616;阶段 B raw-probe spike commit 45b66b0;阶段 C-1 commit 6568e27;C-2a commit 4ad6341;C-2b-dev commit 6ea5b52;C-2b-prod commit fb5062f+c954865;docs 实体 commit 71a0b7f。mingw-w64(WinLibs gcc 16.1.0)已装,不在系统默认 PATH(gnu 工具链编译前提)。
- 核心张力(方案承重,未变):① sidecar 移动端走不通(iOS/Android 沙盒禁子进程,PSD 模板本质桌面专属)② LGPL 纯 Rust 库静态链接进闭源合规未裁(收费与否都有义务),唯 LibRaw CDDL 干净。
- **里程碑:RAW prod 打包链闭合**(externalBin + staging 按打包 host triple 落名 + prod 选路 current_exe 同目录,cargo check debug+release 绿/installer 单测 8 passed/`tauri build --debug` 出 MSI+NSIS)。
- 下一步:真机/CI tail(CI gnu job/样张厂商实测/mac-Linux 交叉编译/GUI 真机验收/release 实跑取证,详见 task_plan「剩余工作」)。

## 回顾(收口时填)
- 亮点:
- 教训:
- 意外:

## 会话:2026-07-24
- 做了:建三件套骨架(手工,worklog-kit CLI 不可用因 D: 盘故障/未装);派 4 路侦察+调研子代理并发摸底 PSD 插件模板 + RAW 库 landscape。
- 验证:(阶段 0 无施工,无门禁)
- 遗留:阶段 1-4(方案/裁决/施工/收口)全待阶段 0 落盘后推进。
- 方案定案:exotic builtin distribution + 桌面 sidecar(LibRaw/CDDL,嵌入预览)+ 移动端二期 in-process,分期 A-E(见 task_plan「方案」节)。
- 用户三裁 + 2 技术定案共 5 条登记为 D-427..D-431(task_plan「关键决策」)。
- 待批:阶段 A(P0)/阶段 B(P1)开工;findings 已补结构化发现 + 外部资料 + F-037..F-040 耐久候选。
- P0 施工完成:commit e28c616,7 文件(mediaGrid.helpers.ts+spec/MediaThumb.vue/ContentViewer.vue/i18n zh-CN+en-US/eslint.config.js)。
- 验证:vitest 48 passed / vue-tsc 0 / eslint clean。
- 复核:cavecrew-reviewer 7 点核对全过,无发现。

## 会话:2026-07-24(阶段 B + C-1)
- 阶段 B:raw-probe 去险 spike——证 rsraw=0.1.1(vendored LibRaw C++)在 x86_64-pc-windows-gnu 工具链下 `cargo build` 绿,绕开 rsraw 在 MSVC 工具链上的硬 panic(worker 是独立 sidecar 进程,不与 Tauri host 共享 ABI,故 gnu-only 可行);commit 45b66b0;复核无发现;vendor 真实解码实测 gated on `RAW_PROBE_SAMPLES`(仓内无样张,未跑)。
- 阶段 C-1:落地架构 A(builtin 叠加豁免)+ 新建 raw-worker crate;commit 6568e27;`cargo test --lib` 965 passed / 0 failed;opus 深审两轮——第一轮抓出 P0(catalog 冲突逻辑打死整个 exotic 子系统,16 测红)、第二轮增量核对 W 修复 + H1 契约,全过。
- 发现两处原假设被证伪(详见 task_plan「计划修正」):raw-worker 不可作根 workspace 默认 member(rsraw MSVC panic);exotic-ocr builtin 先例不成立(能力标记非真扩展名),RAW 是全仓首例触发 CommonFormatConflict。
- 遗留:阶段 C-2 进行中——mod.rs:408 授权门 + fast_scan.rs:552/600 播种守卫两 P1 挡死待修,修完再交子代理做 worker 交付。

## 会话:2026-07-24(续,阶段 C-2)
- C-2a(commit 4ad6341):修 opus 审出两 P1 挡死——授权门(`mod.rs` `availability_of` 加显式 `license_tier=="free"` → Authorized,fail-closed)+ 播种门(`fast_scan.rs` 谓词改 `!builtin || classify_media_type(fmt).is_some()`,cr2 播种/ocr 不播种)。回归测 966 passed 全绿。
- C-2b-dev(commit 6ea5b52):`resolve_worker_path` 加 builtin 分支——dev 模式按 `plugin_id` 分发 `EXOTIC_RAW_WORKER_PATH`(镜像 PSD 旁路),prod 暂 warn+None 拒起不 panic。单测 967 passed 绿。
- **里程碑:RAW 在 dev 端到端接线完成**(cr2 识别→catalog 叠加路由→免费授权→播种→dev worker 定位→raw-worker 解嵌入预览 WebP)。
- 遗留:C-2b-prod(tauri externalBin 跨工具链打包)待真机/release 验证;CI gnu job 未建;样张厂商实测未跑;mac/Linux 交叉编译未验;GUI 真机验收待做。

## 会话:2026-07-24(续 2,C-2b-prod + docs 线实体)
- docs 裁决:worklog-kit upgrade 已播种(实体在盘未提交,CI 视角门红),补使命后随 R2/侦察件入库(commit 71a0b7f),门禁绿 497 文档。
- C-2b-prod 施工(commit c954865):externalBin + staging 脚本(按打包 host triple 落名)+ prod 选路(current_exe 同目录,镜像 ai-worker 先例)+ release.yml(rustup target add + dlltool/g++ fail-fast 探测)。
- 计划外兜底臂 fix(commit fb5062f,单独提交):enhance 三变体漏更(F-056 现象实例,详见 findings)。
- 验证与深审结论:1P1(dev 硬失败改三分支 fail-soft)已修 + 增量核验过;release-only cfg 分支补 `cargo check --release` 关窗;cargo check debug+release 绿;installer 单测 8 passed;`tauri build --debug` 出 MSI+NSIS,raw-worker.exe 复制到 app exe 同目录。
- 环境说明:WinLibs mingw64/bin 不在系统默认 PATH,本机长期跑 `build:raw-worker` 需临时前置。
