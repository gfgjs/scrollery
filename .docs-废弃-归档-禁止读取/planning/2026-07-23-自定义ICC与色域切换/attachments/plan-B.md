---
id: 2026-07-23-plan-B
status: active
type: plan
line: 自定义ICC与色域切换
created: 2026-07-23
---

# B 线实施计划:查看器渲染色域(architect 产出,主线已批)

> 主线裁决(2026-07-23):**批准照施工**。task_plan 待裁项「B 派生回传=文件 vs 内存」就此裁定=**文件**(§5 已弃方案第 3 条)。排程:组 F 先行,组 R 待 A 批 commit 后开(与 A 同碰 editing/color.rs,不并发)。§7 顺手发现已主线证伪(磁盘 grep 无 `\ ` 前缀,工具渲染伪象,不立案)。

前提(已钉死,本计划不复议):D-411 Rust 派生渲染嵌 target ICC;D-412 像素域=嵌入 profile;D-413 编辑链 sRGB 不动;D-414 桌面先行、非桌面锁 sRGB;sRGB=直显原图零派生;范围=ContentViewer 大图,缩略图归 A 线。

---

## 0. 十项裁决

**① IPC 命令面**
- 新文件 `src-tauri/src/ipc/viewer_color_commands.rs`,四命令:
  - `get_viewer_color_url(item_id: i64) -> Result<Option<String>>`:`None`=直显原图(target=srgb / 移动端 / 非派生适用),`Some(abs_path)`=派生文件绝对路径(正斜杠,镜像 `media_commands.rs:189`)。target **不由前端传参**,后端从 `ConfigManager`(`config/mod.rs:129` `get()`)读 `viewer_color_target`+`viewer_color_custom_id`——config 单源,防前后端口径分叉;派生路径含 target_id,URL 天然随 target 变化。
  - `import_icc_profile(file_path: String) -> Result<IccProfileInfo>`、`list_icc_profiles() -> Result<Vec<IccProfileInfo>>`、`delete_icc_profile(profile_id: String) -> Result<()>`;`IccProfileInfo { id: String /*16-hex*/, name: String, file_size_bytes: u64 }` serde camelCase。
- 错误码:`error.rs` 新变体 `AppError::Color { code: &'static str, message: String }`(同 `Reveal`/`Edit` 姿态,`error.rs:90-175` 先例;Serialize 透传 arm 加在 `error.rs:368-387` 段)。稳定码集:`icc_parse_failed` / `icc_not_rgb` / `icc_not_display_class` / `icc_transform_unsupported` / `icc_too_large` / `icc_io` / `icc_not_found` / `unsupported_platform` / `viewer_render_unsupported` / `viewer_render_decode_failed` / `viewer_render_too_large` / `viewer_render_io`。message 只写中文+英文提示语,**不携带路径与底层错误串**(error.rs:114 先例)。
- spawn_blocking:DB 读+解码+CMS+编码+落盘整段进 `tokio::task::spawn_blocking`,完全镜像 `media_commands.rs:141-209`(`get_companion_video_url`,含卷离线守门 :155 与原子写 :200)。
- 并发去重:`state.rs:27` `AppState` 新增 `viewer_render_locks: Mutex<HashMap<(i64, String), Arc<tokio::sync::Mutex<()>>>>`(std Mutex 只护 map、取出 Arc 即释放,**不跨 await**;per-key tokio Mutex 跨 spawn_blocking await 持有——硬约束允许的「unavoidable」场景,注释写明论证)。流程:锁 map→get_or_insert Arc→放 map 锁→`.lock().await`→命中检查/渲染→释放→回锁 map,`Arc::strong_count==1` 时移除条目。构造点 `state.rs:392-439`。

**② 派生缓存**
- 目录与键:`{cache_dir}/viewer_color/{target_id}/{2-char-prefix}/{cache_key_hex}.{jpg|png}`。`cache_key` 复用既有值(`utils/hash.rs:29`,已含 mtime——mtime 变→key 变→旧文件成孤儿归 GC)。`target_id`:`display-p3` / `dci-p3` / `icc-{profile_id}`(profile_id=ICC 原字节 xxh3 16-hex)。路径助手加在 `thumbnail/cache.rs`(`thumb_path` :19-33 同款)。
- 原子写:复用 `thumbnail::generator::write_atomic`(tmp+同卷 rename;命中判定只查 exists())。
- 记账衔接(2026-07-19 裁决延伸):viewer_color **并入 10GB 单源 LRU 预算**:`enforce_cache_limit` 的 `scan_dirs`(cache.rs:199-203)加 `viewer_color`;被驱逐路径不在 `thumbnails/` 下→`evicted_thumb_db_path`(:175)天然返 None,无状态复位需求。`CacheStats`(:305-324)加 `viewer_color: CacheCategoryStat` 并计入 total(:347-363);`cache_subdirs_for_kind`(:366)加 `"viewer" => &["viewer_color"]` 并入 catch-all;`derivations_to_reset_for_kind`(:392)加 `"viewer" => (&[], false)`;`reconcile_orphan_gc` 子目录清单(:522-529)加 `"viewer_color"`(walkdir 递归穿 target_id 层,16-hex stem 护栏适配)。`cache_files_for_key`(:451)**不扩**——target 目录动态,纯函数枚举不完备,孤儿归对账 GC;注释与测试(:604 `len==14` 保持)写明理由。

**③ 输出编码**
- image crate `JpegEncoder`/`PngEncoder` + `set_icc_profile`(先例 `editing/io.rs:57-70`)。有 alpha→PNG(RGBA 直编);无 alpha→`to_rgb8()` 后 JPEG quality 92(edit_commands.rs:47 同档;JpegEncoder 不收 RGBA)。100MP:`editing::memory_budget` 同款峰值预算门,超限 `viewer_render_too_large`。不写 EXIF,**orientation 必须烤进像素**(`editing::metadata::read_source_metadata` + `apply_orientation`,io.rs:85 先例)。16-bit/f32 经 CMS 后量化 8-bit(注释说明)。
- target ICC 字节:内置=`ColorProfile::new_display_p3()`/`new_dci_p3()`(moxcms defaults.rs:283/:343)+ `.encode()`(已核实:moxcms-0.8.1 writer.rs:646);自定义=`config/icc/{id}.icc` **原字节**(绝不重编码)。CMS 变换与嵌入用**同一 profile 对象/同一字节**→D-412 构造性成立。
- CMS 泛化:`editing/color.rs` 把 `to_srgb_rgba8`(:33-133)的 match 抽为 `pub fn to_target_rgba8(image, icc: Option<&[u8]>, target: &ColorProfile)`,`to_srgb_rgba8` 变薄封装(target=`new_srgb()`),intent 仍 `options()`。行为差异一处:**target≠sRGB 时无 ICC 源假定 sRGB 仍须转换**(viewer 侧 None→source=`new_srgb()`;编辑侧 None→原样返回,现契约不动,D-413)。既有 lcms2 对拍测试(:276-349)原样通过=编辑链未漂移守卫。

**④ 自定义 ICC 导入**
- 命令入 `viewer_color_commands.rs`;持久化 `{app_data_dir}/config/icc/{id}.icc`(state.rs:117-120;tmp+rename)。
- 校验链(spawn_blocking 内):canonicalize+`is_file`→大小 ≤16MB(`icc_too_large`)→`new_from_slice` 失败=`icc_parse_failed`→`color_space != Rgb`=`icc_not_rgb`→`profile_class != DisplayDevice`=`icc_not_display_class`→**变换探针**:`new_srgb().create_transform_8bit(Layout::Rgba, &candidate, Layout::Rgba, options())` 失败=`icc_transform_unsupported`(LUT 型导入时拦截)。name 取 desc tag,缺省 `ICC {id 前 8 位}`。同字节重复导入幂等。
- `delete_icc_profile`:id 严格 16 位小写 hex(防路径注入,先例 cache.rs:487-500)→删 `.icc`→删 `{cache_dir}/viewer_color/icc-{id}/`→若 `viewer_color_custom_id` 恰为该 id,后端复位 `viewer_color_target=srgb`、清 custom_id。
- capabilities:**无需新增**——自有命令归 `core:default`,`dialog:default`+`dialog:allow-open` 已在(capabilities/default.json:8-9),派生与 icc 目录均在 `$APPDATA/**` scope(tauri.conf.json:45-48)。dev 下 import 弹窗可用性归 GUI 手测。

**⑤ 设置链**
- schema(`config/schema.rs:403` 后新 `// ── viewer ──` 组):`viewer_color_target`:`SettingKind::Enum(&["srgb","display-p3","dci-p3","custom"])`(先例 :350),default `"srgb"`,hot=true;`viewer_color_custom_id`:`SettingKind::Str`,default `""`,hot=true。state.rs:36 键数注释同步;config 键数断言测试(若有)一并更新。
- configStore:state 加 `viewerColorTarget`/`viewerColorCustomId`(:8-38);`loadConfig` 两行 fetchStr(:57-76);setter 两个(:182-189 同款,**不调 restartDerivation**)。
- useConfigFile.ts:**零改**(`DERIVATION_RESTART_KEYS` 不加)。
- SettingsView 四段:settingsMap.ts `viewerColorTarget` select(section 'general')+ `viewerIccManager` customRow(:61-62 契约);DynamicSettingControl.vue:208-256 selectBindings 一条;SettingsView.vue custom row=导入按钮(plugin-dialog `open({filters:[{name:'ICC',extensions:['icc','icm']}]})`→`IMPORT_ICC_PROFILE`)+ 已导入列表(单选+删除;选中→`setViewerColorCustomId(id)`+`setViewerColorTarget('custom')`);两行 `v-if="!isMobilePlatform"`(utils/platform.ts:40)。CacheStatsPayload(:614-616)加 `viewerColor`+统计行(:645)。i18n 全语言对齐(localeIntegrity.spec.ts 强制)。

**⑥ 前端换源状态机**
- 新 `src/composables/useViewerColorSource.ts`:输入 `detail`(id/mediaType)、configStore、`isMobilePlatform`;输出 `displayUrl: Ref<string|null>`。
- 状态机(**原图先显+完成换源**):`watch([detail.id, target, customId])`→移动端/srgb/非 image→`null`;否则保持当前显示,发 `GET_VIEWER_COLOR_URL`,返程**过期守卫**(item/target 已变→丢弃;先例 ContentViewer.vue:611-629 poster watch),成功→`resolveAssetUrl(path)` 换源,失败→`null` 静默回退原图(logger 一条)。设置热更走同一 watch。
- `ContentViewer.vue:590`:`absPath` = `viewerColor.displayUrl.value ?? (detail.value ? resolveAssetUrl(detail.value.absPath) : '')`(同 computed 喂 img/video/audio,非 image 恒 null 行为不变;先例 liveVideoSrc :1333)。
- 派生 URL 加载失败兜底(LRU 驱逐竞态):`onMediaError` 前置分支——src 为派生 URL 时复位 `displayUrl=null` 重试原图一次。
- EditOverlay(:42-48)吃原图路径不经 absPath——编辑链天然不受影响(D-413),复核确认。

**⑦ 平台门控(D-414)**:后端 `#[cfg(any(target_os="android", target_os="ios"))]`——render 返 `Ok(None)`,import/list/delete 返 `unsupported_platform`(先例 ipc/reveal.rs:37-41);前端软闸见⑤⑥。

**⑧ 测试清单**
1. `viewer_color/target.rs`:target_id 映射;内置 profile `encode()` 字节可 `new_from_slice` 回读且 `color_space==Rgb`。
2. `viewer_color/render.rs`:缓存路径布局;alpha→png、不透明→jpg;orientation 烤入(合成 JPEG→输出复读 NoTransforms,镜像 io.rs:80-90)。
3. **特征测试(全线最值钱)**:AdobeRGB 合成源(lcms2 构造,color.rs:316-349 先例)→Display P3 渲染→复读输出断言 (a) 嵌入 ICC == `target.encode()`,(b) 像素与 lcms2 AdobeRGB→P3 参考 ≤2 code value——一条钉死 D-412 两半。
4. `viewer_color_commands`:import 坏字节/Gray profile 错误码、序列化快照(reveal.rs:59-64 先例);import→list→delete 闭环;删除当前选中→config 复位。
5. cache.rs 既有测试更新:stats_sum_per_subdir(:638)、clear kind "viewer"(:659)、orphan GC(:678);`enumerates_all_artifacts_for_key`(:604)保持 14 并注明。
6. 回归守卫:editing/color.rs 既有测试原样绿(D-413)。
7. `useViewerColorSource.spec.ts`(前端最值钱):srgb 不发 IPC;p3 换源;请求中切 item/target 丢弃;失败回退;mobile 恒 null。
8. settingsMap 键集由 vue-tsc satisfies 对账兜底。

**⑨ 施工拆分**:组 R 全在 `src-tauri/**`,组 F 全在 `src/**`,零共享文件;IPC 契约本计划钉死。R 12–16 轮,F 8–10 轮。
**⑩ 风险**:见 §6。

## 1. 改动清单(逐文件)

### Rust 域(组 R)
| 文件 | 改动 |
|---|---|
| `src-tauri/src/viewer_color/mod.rs`(新) | 模块声明+顶部文档(契约、D-411/412/414) |
| `src-tauri/src/viewer_color/target.rs`(新) | `ViewerColorTarget` 解析、target_id、内置 profile 构造+encode 字节、自定义字节读取;测 ⑧-1 |
| `src-tauri/src/viewer_color/render.rs`(新) | 缓存路径助手、命中检查(jpg→png)、渲染主函数:解码(ext 白名单同 edit_commands.rs:32;动画 webp→`viewer_render_unsupported`)→memory_budget→orientation 烤入→`to_target_rgba8`→ext 判定→编码嵌 ICC→write_atomic;测 ⑧-2/3 |
| `src-tauri/src/ipc/viewer_color_commands.rs`(新) | 四命令(①④;render 镜像 media_commands.rs:141-209:卷离线守门→DB 取 path+cache_key→keyed lock→命中/渲染);cfg 移动分支;测 ⑧-4 |
| `src-tauri/src/ipc/mod.rs` | `pub mod viewer_color_commands;` |
| `src-tauri/src/ipc/registry.rs:47-53` | 四命令注册 |
| `src-tauri/src/lib.rs:54` 附近 | `pub mod viewer_color;`(字母序) |
| `src-tauri/src/error.rs` | :159 后加 `Color` 变体+码集文档;Serialize arm(:368-387 段) |
| `src-tauri/src/state.rs` | :27 加 `viewer_render_locks`;:392-439 构造;:36 键数注释 |
| `src-tauri/src/config/schema.rs:403` 后 | viewer 组两键 |
| `src-tauri/src/thumbnail/cache.rs` | :199-203 scan_dirs、:305-324+:347-363 CacheStats、:366-382+:392-411 kind 映射、:522-529 GC、:451 注释;测 ⑧-5 |
| `src-tauri/src/editing/color.rs:33-133` | 抽 `to_target_rgba8`,`to_srgb_rgba8` 薄封装;必要时 metadata.rs `read_source_metadata` 提 `pub(crate)` |

### 前端域(组 F)
| 文件 | 改动 |
|---|---|
| `src/constants/ipc.ts:50` 附近 | `GET_VIEWER_COLOR_URL`/`IMPORT_ICC_PROFILE`/`LIST_ICC_PROFILES`/`DELETE_ICC_PROFILE` |
| `src/stores/configStore.ts` | state 两键/loadConfig 两行/两 setter |
| `src/composables/useViewerColorSource.ts`(新) | 状态机(⑥) |
| `src/composables/useViewerColorSource.spec.ts`(新) | ⑧-7 |
| `src/components/media/ContentViewer.vue` | :590 absPath 接 displayUrl;onMediaError 前置分支;引入组合式函数 |
| `src/constants/settingsMap.ts:74+` | 两项注册 |
| `src/components/settings/DynamicSettingControl.vue:208-256` | selectBindings 一条 |
| `src/views/SettingsView.vue` | ICC 管理 custom row;CacheStatsPayload+统计行;移动端 v-if |
| `src/i18n/locales/*.json` | settings/错误提示键全语言对齐 |

## 2. 并行分组
- 组 R ∥ 组 F 文件域不相交;IPC 契约已钉死。
- 组 R 内序:error.rs+state.rs+schema.rs+editing/color.rs(地基)→viewer_color 模块→cache.rs→ipc+registry+lib.rs。
- 组 F 内序:ipc.ts+configStore→组合式函数+spec→ContentViewer→settings UI+i18n。
- 主线排程备注:组 R 与 A 批同碰 editing/color.rs,待 A commit 后开。

## 3. 边界情况(16 条,施工照办)
1. 无 ICC 源+非 sRGB target:假定 sRGB 仍转换并嵌 target;target=sRGB 恒零派生。
2. CMYK JPEG:解码器已出 RGB(color.rs:42-44)→按无 tag sRGB→转 target。
3. Gray 源+Gray profile:沿用 (Gray,Luma*) 布局匹配,仅换 target。
4. 布局与空间不符(color.rs:131)→`viewer_render_decode_failed`→前端回退原图。
5. Orientation 烤入像素,输出不带旋转指令。
6. alpha→PNG;JPEG 前必转 RGB;16-bit/f32 量化 8-bit。
7. 动画 webp→`viewer_render_unsupported`;gif 不在白名单。
8. 100MP:memory_budget 准入,超限回退原图。
9. 卷离线→`VolumeOffline`→回退原图(走既有错误 UI)。
10. 同 item+target 并发→keyed lock 单渲;异 target 并行。
11. 命中与前端加载间被 LRU 驱逐→onerror 兜底回原图一次。
12. 删除当前选中 profile→复位 config+清派生子树;前端刷新 store。
13. 重复导入幂等;id 16-hex 才可删。
14. mtime 变→新 key→旧派生孤儿→对账 GC 收敛。
15. DCI-P3 白点≠D65:relative colorimetric 白映白,预期;i18n 写「主要用于影院素材核对」。
16. config 键数断言随 schema 更新。

## 4. 验证方式
- 组 R 增量:`cargo check`;`cargo test viewer_color` / `editing::color`(D-413 回归)/ `thumbnail::cache`;涉改 clippy;⚠仓坑:cargo fmt 传文件参数仍重排整 crate,勿跑。
- 组 F 增量:`npx vitest run src/composables/useViewerColorSource.spec.ts src/i18n/localeIntegrity.spec.ts`;`npx vue-tsc --noEmit`;涉改 ESLint(`npm run lint:fix`,禁 repo-wide format)。
- 批末(phase-closer):全量 cargo test+clippy+vitest+vue-tsc(quiet);GUI 手测清单(四 target 换源色变/ICC 导入删除/缓存统计行/移动端锁 sRGB——not automated)。

## 5. 已弃方案(不复议)
canvas colorSpace / force-color-profile 特性面;DerivationKind 后台流水线(预算爆炸);内存 packet 回传(裁=文件);media_derivations 记行(exists() 即命中);前端传 target(config 双源);单独 icc_commands.rs;cache_files_for_key 扩 viewer_color;WebP 输出(编码器不支持 set_icc_profile);单键复合值;sidecar 元数据;导入免探针。

## 6. 开放问题(验收期处置)
1. **WebView2 是否严格对嵌 P3/DCI-P3 ICC 的 JPEG/PNG 做 target→display**——真机核;若忽略,特性语义塌方,回主线裁决。
2. 自定义 LUT 型 profile 探针通过但精度差——真机目测;「不保真」免责文案留验收裁。
3. 100MP 全分辨率派生性能——先按全分辨率,不可接受再回裁,勿擅自降级。
4. `viewer_color_custom_id` set 层存在性校验——留复核裁。
