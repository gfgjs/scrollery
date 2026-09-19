---
status: snapshot
type: working-memory
line: 视频播放器重构
created: 2026-07-22
---

# 视频播放器重构 · progress

- 2026-07-22 开工建卡。P0a(仓内摸底)+ P0b(联网调研)双代理并发派出,等回执。

## 全批次日志

- **P0**:摸底(ContentViewer 原生 video 裸播)+ 调研(UI 库/编解码/出路矩阵)完,见 findings.md。
- **P1**:用户裁 UI 自研 Vue 控件;档位按轻档先行(三档公共前缀);remux/原生解码=收口裁决点。
- **P2**:architect 计划定稿(四项裁决:←→归 seek、V23+localStorage 分储、前端截帧不 taint、sprite 复用)。
- **G0** `523153d`:eventToCombo shift+非字符键,11 测绿,haiku 复核零发现。
- **GB** `1bbc55b`:三 utils+23 测;复核 1🔴(cue 空行)主线驳回——SRT/VTT 空行即 cue 终结符;1🟡 属性标签失配已修。
- **GA** `047dbb0`:V23 全链+player IPC 两命令+19 测;opus 深审 0 严重(capabilities 走仓内 registry 白名单机制核实无缺),1🟡6🔵3存疑逐项裁决:大小写/vtt 优先/TOCTOU/魔数+上限/Option 映射/轻查询采纳修,授权模型裁「对话框即授权」,`.ok()` 吞错维持既有约定;增量核验全过。
- **GD** `fae8d7d`:14 命令+when 收窄,45 测绿,haiku 复核零发现;快捷键契约变更:视频上下文 ←→ 归 seek,切条目 Shift+←→。
- **GC+GE+修复** `4dccab9`:播放器七组件+八 composables+ContentViewer/viewerStore/i18n;opus 深审 0 严重 3建议4存疑全裁决落地(HEVC 须 codec 实证/Infinity seekable 兜底/连 F 竞态回读/prefs 时序/wheel.stop/pointercancel/切条目清字幕轨);codec 值域 scout 取证:MF 友好名 "HEVC",fourcc 缺口不成立不修;增量核验全过。
- **GF(本批)**:批末门禁 + oversize 测试增补。

### GF 门禁结果

- `cargo test player_commands`(src-tauri):17 passed(新增 `rejects_base64_over_max_bytes` 1 例),exit 0。
- 前端 `npm run test -- --run`:首轮 1 red(主题契约测发现幽灵 token `--color-bg-tertiary`,消费于 `VideoRateMenu.vue`/`VideoSubtitleMenu.vue` 死回落 fallback,六套主题均未定义),定位为本线笔误(`--color-bg-hover` 才是六主题均定义的真实 token,`--color-bg-tertiary` 是不可达的死 fallback)→ 改为 `var(--color-bg-hover)` 单值,复跑绿:114 files / 1389 tests passed,exit 0。
- 前端 `npm run typecheck`:exit 0,无输出即通过。
- 前端 `npm run lint`:exit 0,无输出即通过。
- 后端 `cargo test --workspace`(quiet):全绿,主 lib 套件 907 passed(较修前 906 +1,6 ignored,与本线无关的既有 ignore 不变),exit 0。
- 后端 `cargo clippy --workspace --all-targets`(quiet):零警告,exit 0。
- 后端 `cargo fmt`:重排面仅落 `src-tauri/src/ipc/player_commands.rs`(白名单内);同时波及 `src-tauri/src/db/queries/derivations.rs`(不在白名单)→ `git restore` 撤销,不吸收进本线。

## 遗留/手测清单(标注 not automated,GUI 真机)

① 普通 mp4 带 crossorigin 可播(首验项!若 CORS 黑屏则截帧改 blob URL 二期);② mkv 探针矩阵日志裁决争议;③ HEVC 无扩展机诊断文案;④ autoplay 拦截覆层;⑤ PiP 进出+切条目自动退;⑥ 全屏对 F/Esc 与 F11 互不越权;⑦ 截帧(Windows 先行,macOS taint 未实证);⑧ mp4 内挂字幕轨 WebView2 可见性;⑨ sidecar srt 自动加载+GBK 编码;⑩ 倍速/拖拽手感/位置记忆跨会话;⑪ 音量滑杆滚轮不翻页;⑫ save_frame_png tmp 失败清理分支(注入写失败难自动化)。

## 未决裁决点(留给用户)

A) remux/原生解码二期是否立项;B) worklog 收口归档待用户点名(D-014 status 分片走向届时定);C) viewer-audio Space 键位统一邻接后续;D) 七 commit 待批 push。

## 回顾

摸底(P0)证实播放器本体是单个原生 `<video controls>` 裸播,零定制;联网调研(P0b)排除 GPL 系方案(libmpv 默认 GPL 需专门构建选项才 LGPL)、确认 WebView2/HEVC 扩展依赖与 mkv 支持争议无法静态裁定。用户裁自研 headless + 轻档先行,architect 出四项关键裁决(快捷键契约/存储分层/截帧不 taint 前提/sprite 复用)锚定全部下游施工,全程无返工。六批施工(G0/GB/GA/GD/GC+GE/GF)按依赖 DAG 三路并发起步,写域不相交处全程并发,两轮 opus 深审(GA 后端、GC 前端核心)共发现 1 严重 0/8 建议存疑,逐项裁决落地且增量核验全过,无一处走"复核发现即返工重来"的弯路。唯一返工:GC-fix 顺手发现 HEVC fourcc "缺口"经 scout 取证证伪(本仓值域是 MF 友好名非 fourcc),避免了一次不必要的代码改动。

七 commit(六施工+一门禁收口)全绿收口,零基线红污染;12 项 GUI 真机手测与二期 remux/libmpv 立项留待用户后续裁决。
