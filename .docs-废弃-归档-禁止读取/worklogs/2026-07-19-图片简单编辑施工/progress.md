---
status: 快照
type: working-memory
line: 图片简单编辑施工
created: 2026-07-19
---

# 进度日志:图片简单编辑施工

<!-- 验证行怎么填(F-005):门禁末行须在**全部内容落盘后**才跑得出来——顺序是
     先写占位 → 跑门 → 回填真实末行(改动了再复跑)。别倒过来抄一行旧输出充数。 -->

## 会话:2026-07-19
- 做了:读方案 C 定稿;对 HEAD 核实 image/webp 版本、view_rotation、RunTokenSlot/file_job_owner/
  background_heavy_limiter、AppError 范式、查看器入口位置,确认无实质性代码漂移;建施工三件套;
  采纳 C-1..C-6 建议列为正式决策。
- 做了:阶段 1 P0 元数据/色彩 spike——读 `image 0.25.10`/`webp 0.3.1`/`kamadak-exif 0.5.5` 三个
  vendored crate 源码(非猜测 API),新建 `src-tauri/src/editing/{mod.rs,metadata.rs}`:
  `read_source_metadata`(orientation/icc/DateTimeOriginal 读取,PNG/JPEG 通用)+
  `build_output_exif`(手工最小 TIFF/EXIF blob,Orientation 固定 1 + 可选 DateTimeOriginal,不转发
  原始 blob)。锁定 D-005(不引入 img-parts/little_exif)、D-006(WebP 输出 v1 不 admit)、
  D-007(输出侧自建最小 EXIF 而非编辑原始 blob)。
- 验证:`cargo test --lib editing::` 6/6 通过(jpeg/png golden 往返、orientation 归一化+丢弃缩略图、
  非方形样本宽高互换证据、无日期/非法日期降级两态字节长度断言)。`cargo clippy --lib -- -D warnings`
  在未改动的 `ipc/scan_commands.rs:96` 报 pre-existing `manual_inspect`(与本任务无关,`git diff` 确认
  零改动该文件);追加 `-A clippy::manual_inspect` 后确认 `editing::metadata` 零告警。
- 做了:阶段 2 P0 内存/并发基准——新增 `examples/edit_memory_probe.rs`(合成源 + 独立进程测
  `GetProcessMemoryInfo` PeakWorkingSetSize 增量,新增 Cargo feature `Win32_System_ProcessStatus`)
  在 24/50/100MP + 40MP 长边 × jpeg/png/webp × decode/full 共 22 组实测(webp 长边因格式单边尺寸
  上限报错,记 F-004);JPEG 在 50MP/100MP 两点 ≈2.95 字节/像素线性一致,采信为格式无关基线。
  落地 `editing::memory_budget`(预算系数 6 字节/像素、上限 1.4 GiB、`predicted_peak_bytes`/
  `exceeds_memory_budget`、稳定码 `edit_image_too_large`)+ 4 单测。锁定 D-008。
- 验证:`cargo test --lib editing::` 10/10 通过;`cargo clippy --lib -- -D warnings
  -A clippy::manual_inspect`(排除 F-003 记录的既存无关告警)零告警。
- 遗留:阶段 3 后端契约(`AppError::Edit` + `EditOps`/`save_edited_image` + 几何链 + 原子落盘 +
  单文件 ingest)。
- 做了:阶段 3 起步——`AppError::Edit{code,message}`(方案 §6 八码,与 Backup/Export 同姿态)+
  `editing::geometry`(纯函数几何链:orientation 只应用一次 → 合入 view_rotation 与本次 rotate
  取模 360 → flip → crop,crop 在变换后坐标系直接 clamp 不用反推,零面积拒 `edit_crop_empty`)。
- 验证:`cargo test --lib editing::` 18/18 通过;clippy 零告警(同排除项)。
- 遗留(阶段 3 剩余,读了 `scanner/enricher.rs` 但还没读 `fast_scan.rs`/`walker.rs` 的
  insert-new-item 路径):单文件 `ingest_single_file`(directories 表 upsert/`tree_sort_key`
  计算/content_hash/去重/缩略图队列触发,这段直接碰扫描索引不变量,需要先读完 fast_scan 再动手,
  比前面几段风险高)、原子临时文件写 + 元数据复读验证 + rename、`save_edited_image` IPC 命令 +
  AppState 前台编辑门闩(复用 file_job_owner 姿态)+ registry 注册。frontend 全部(useImageEditor.ts/
  EditOverlay.vue/入口接线)未开始。

## 会话:2026-07-19(复审)
- 做了:对最近 4 commit(0177f68..0525dde)复审。**决定性发现**:内存基准 JPEG 2.95 B/px 读数
  同受 setup 伪影污染(测量进程内合成源图预提交页面,与首轮已识别的 PNG/WebP 伪影同源,当时
  未对 JPEG 用同一标准)。探针加 `--emit-source`/`--source-path` 两段进程模式复测,24/50/100MP
  三点均 ≈6.02 B/px(decode ≈3.17,主峰=rotate90 双缓冲),首轮「系数 6 ≈ 2 倍余量」实为零余量。
  按 D-008 既定「实测最坏 × 2」政策把 `EDIT_PEAK_BYTES_PER_PIXEL_BUDGET` 6→12(准入上限 125MP,
  仍覆盖 100MP 样本),同步改写 memory_budget 模块文档、findings(F-003 第二层 + 新实测表 +
  旧口径划线推翻)、task_plan D-008。
- 做了:geometry 小修——`apply_geometry` 加 debug_assert 钉「ops.rotate 校验责任在命令层」
  (release 仍取模防御不 panic);原 `flip_applied_after_rotation` 测试名不副实(只测了镜像没测
  顺序),改名 `fliph_mirrors_horizontally` 并新增 `flip_is_applied_after_rotation_not_before`
  (rotate90+fliph=转置的像素级顺序判别);探针 decode 改借用源字节免复制入 delta。
- 验证:`cargo test --lib editing::` 19/19 通过;`cargo clippy --lib --example edit_memory_probe
  -- -D warnings -A clippy::manual_inspect` 退出码 0。
- 遗留:同上会话(阶段 3 剩余不变);另有 4 项需用户裁决(见会话报告):PNG 族 orientation 与
  view_rotation 语义冲突(阶段 3 开工前必须裁)、非法 rotate 的 IPC 响应姿态、ceiling 1.4 GiB
  设备预算复核、若期望准入 >125MP 需明示降余量。
- 用户裁决(「全部采纳建议」):D-009 编辑链 orientation 仅对 JPEG 生效(对齐 viewer,防双重
  旋转),落地 `metadata::effective_source_orientation` + 单测;D-010 新增稳定码
  `edit_invalid_ops`,校验前置进 `apply_geometry` 入口(替换 debug_assert,release 也拒),
  error.rs 码集文档同步;D-008 附注:维持 ceiling 1.4 GiB + 125MP 准入,复核仍待设备预算输入。
- 验证:`cargo test --lib editing::` 21/21 通过;clippy(同排除项)退出码 0。

## 会话:2026-07-19(阶段 3 收尾 + 阶段 4 前端,续接上会话遗留)
- 做了:阶段 3 剩余全部落地——新建 `editing::naming`(目标 stem 派生 + `claim_target_path`
  原子认领,复用 `export::naming::sanitize_component` 做字符净化,不重复造轮子)、
  `editing::io`(`write_edited_image`:编码→写 tmp→磁盘复读验证 orientation 归一→sync_all→
  rename 覆盖占位,任一步失败清 tmp+占位)、`editing::ingest`(`ingest_single_file` 复用
  `upsert_fast_scan_item`,跳过 `ensure_dir_chain` 递归——目标目录即源 item 目录,
  `directory_id` 已知;不播种 exotic 任务,v1 输出恒 jpg/png 非 exotic 格式)、
  `db::queries::scan::get_root_and_volume_for_directory`(新查询,供命令层取 volume_id)、
  `state.rs` 新增 `FILE_JOB_EDIT` 常量(裁 D-011:复用 `file_job_owner`,非新槽——
  `error.rs` 既有文档「A/B/C 共用」已实质定调)、`ipc/edit_commands.rs`(`save_edited_image`
  命令:`FileJobReleaseGuard` RAII 释放门闩;单发直跑不采用 backup/export 的
  job_id+事件模型,D-012;成功后 `bump_data_version` + 后台单根 `run_enrichment`
  fire-and-forget,不阻塞命令返回;`EditSaveResult::Saved{newItemId}` /
  `SavedNeedsIndex{pathHint,recoveryCode,rootId}`)。
- 做了:阶段 4 前端全部落地——`useImageEditor.ts`(几何/状态机 + `requestDiscardEdits`
  共用判定)、`EditOverlay.vue`(预览改 CSS-transform `<img>` 而非方案原文「降采样
  canvas」,D-013——与主查看器 `useMediaDetail.ts` 的 view_rotation 预览同一手法,裁剪框
  包围盒因旋转恒 90° 倍数、纯算术推导,不需要 canvas;8 手柄裁剪,比例锁定时仅 4 角可见;
  三分线;键盘微调接 `ContentViewer.onKeydown`)、`ContentViewer.vue` 接线(底部控制条
  `PencilLine` 按钮;`editor.status!=='idle'` 时隐藏原生控制条 + 拦截翻页/滚轮/方向键;
  `close()`/`navigate()` 双重网关防命令层绕过 Esc 拦截误触发;保存成功
  `media.openDetail(newId,true)` + 路由同步)、`commands/builtins/viewer-image.ts` 新增
  `viewer.edit`(view 组,仅 image)、`viewerStore.ts` `ViewerApi.edit` 字段、
  `constants/ipc.ts` `SAVE_EDITED_IMAGE`、中英文 `edit.*` 词条(含 `common.ok` 复用)。
- 修:`eslint.config.js` i18n 裸文本白名单补 `4:3`/`3:4`/`16:9`/`9:16`/`JPEG`/`PNG`(技术标识,
  非自然语言,与既有 `1:1` 同类);`EditOverlay.vue` 一处 CSS 用了不存在的 `--color-danger`
  ghost token(主题契约门禁抓到),改用既有 `--color-warning`/`--color-error`;
  `commands/builtins/viewer-image.spec.ts` 既有「viewerImageCommands 全体对 image/video
  均可见」断言因新增 image-only 的 `viewer.edit` 被打破,拆出单独用例断言其仅 image 可见,
  不放大到全局豁免。
- 验证:`cargo test --lib`(750/750,较上会话 21 新增至全库)+ `cargo clippy --lib --example
  edit_memory_probe -D warnings -A clippy::manual_inspect`(0)+ `vue-tsc --noEmit`(0)+
  `eslint .`(全仓 0)+ `vitest run`(1253/1253)全绿。
- 遗留(诚实记录,非本次范围内合理完成):`save_edited_image` 命令层集成测试(需要真实
  AppState 脚手架,子模块已充分单测、编排层本身简单,构造成本边际收益低,判断值得但未做)、
  EditOverlay.vue 裁剪拖拽/比例锁的 vitest(pointer 事件模拟成本高,`useImageEditor.ts` 纯函数
  部分本可测但也未补)、方案 §8 前端验证矩阵项(crop 坐标换算/支持态/partial 文案/保存后
  网格跳转均无自动化,仅编译+类型检查)、真机 GUI 交互验收(crop 手感、保存耗时体感、
  跨平台路径分隔符)完全未做。partial 态「立即扫描」按钮已接 `scanStore.startScan(rootId)`,
  但未做端到端验证。

## 会话:2026-07-19(v1 提交前复核)
- 做了:按用户要求暂停 v2 施工，重新界定 v1 提交路径；仅格式化图片编辑相关 Rust 文件，未吸收
  `docs/status/批量15项问题清单.md`、v2 三件套或其他无关工作。
- 验证:`rustfmt --check` 对 v1 明确路径退出码 0；`cargo check --workspace --locked` 退出码 0；
  `cargo test --workspace --locked` 退出码 0（Rust 主库 750/750，workspace 其余测试全绿）；
  `cargo clippy --workspace --locked -- -D warnings -A clippy::manual_inspect` 退出码 0；
  `npm run lint`、`npm run typecheck`、`npm test`（98 files、1258 tests）、`npm run build` 均退出码 0。
- 已知基线门禁:`cargo fmt --all -- --check` 仍被 HEAD 中与 v1 无关的 backup/export 格式漂移阻挡；
  严格 `cargo clippy --workspace --locked -- -D warnings` 仍被未改动的
  `src-tauri/src/ipc/scan_commands.rs:96` `manual_inspect` 阻挡。两项均未混入本次 v1 commit。
- 遗留:v1 自动化矩阵与真机 GUI 缺口维持上节记录；v2 三件套将接续补齐。

## 回顾(收口时填)
- 亮点:
- 教训:
- 意外:
