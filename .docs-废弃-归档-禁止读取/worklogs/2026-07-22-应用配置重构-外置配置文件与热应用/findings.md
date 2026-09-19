---
status: 快照
type: working-memory
line: 应用配置重构-外置配置文件与热应用
created: 2026-07-22
---

# 发现与决策:应用配置重构-外置配置文件与热应用

## 需求
- 用户原话要点:全部设置键值(尽可能全,含设置页未暴露的)迁到带注释的外置配置文件;格式参考 VSCode/Codex/Claude Code,不限 .json;设置页给专业用户「外部编辑器打开配置文件」入口;外部编辑实时热应用;合理建议直接做,拿不准列决策清单等用户裁。

## 发现

### 后端现状(摸底 2026-07-22)
✓consumed→b216dcc
- 存储 = SQLite 表 `app_config`(key/value TEXT),定义 src-tauri/src/db/schema.rs:10-38 + 244-249/448-451/525-529 分段初始化;DAO src-tauri/src/db/queries/config.rs:8-24。
- 键约 55 个:schema.rs 有默认值的 ~25 个 + config_commands.rs:57-99 白名单直通键 ~30 个(默认 None,前端兜默认)。⚠回执自认 partial,施工穷举以 config_commands.rs set_app_config 实际 match + get_startup_config 为准。
- IPC 7 命令:get_app_config / get_startup_config(25键一次往返) / set_app_config(config_commands.rs:143-315,内嵌 per-key 热应用副作用) / get_thumb_cache_dir / get_log_dir / get_cache_stats / clear_cache。
- 既有热应用副作用(迁移时必须原样保留,走同一 apply 路径):thumb_size/thumb_webp_quality/thumb_strategy/thumb_skip_max_kb→更新 state.thumb_config+失效布局缓存+复位生成状态(159-312);thumb_cache_dir→重授 asset scope(252-265);log_level→reload tracing filter(277-286);gpu_engine/ai_hq_cache_enabled→更新 thumb_config;另有 bump_data_version()/wake_exotic() 驱动。
- 依赖:serde_json✓ tauri-plugin-opener✓ tauri-plugin-shell✓;toml/toml_edit/notify/tauri-plugin-store 均无。capabilities:default.json + logs.json。
- thumb_cache_dir/thumb_cache_max_mb 初始化在 lib.rs:321-371 启动段,不在 schema.rs。
- ⚠疑似键演化残留:schema.rs:27 `thumb_quality` 与动态 `thumb_webp_quality`(config_commands.rs:210-250)并存,施工时核实是否同物。

### 前端现状
✓consumed→5b0b925
- 设置页 src/views/SettingsView.vue,6 分区(general/thumbnails/video/aiModels/debug/danger),约 40 控件键。
- configStore(src/stores/configStore.ts:7,18 字段)+ uiStore(src/stores/uiStore.ts:96,外观/语言/布局)双 store,走 GET_APP_CONFIG/SET_APP_CONFIG/GET_STARTUP_CONFIG。
- 无 tauri 事件监听设置变更——纯 store 响应式,外部改动无从感知(热应用要新增事件通道)。
- localStorage 旁路 7 键:settingsCardsExpanded(UI 状态)、scrollery.themeSnapshot.v1(FOUC 缓存)、sortModeUsage(埋点)、scrollery.debug.bucketSpacer / scrollery.debug.renderMode / scrollery.debug.perfProbe(调试)、logWindowStore.presets(预设)。
- package.json 无 jsonc/json5/toml 解析依赖(方案设计成前端零新依赖:文件解析全在后端)。

### 散落硬编码可配置项(候选收录,详表见摸底回执,precision 未逐项复核)
✓consumed→b216dcc+5b0b925
- 容量:edit_peak_bytes_ceiling 1.5GB(editing/memory_budget.rs:28)、max_log_dir_bytes 512MB(logging.rs:20)、journal_size_limit 64MB(db/connection.rs:30)。
- 并发:derive_batch_size 256(derive/pipeline.rs:38)、db_read_pool_size 4/2(db/connection.rs:130-132)、background_heavy_limiter_permits 2(state.rs:436-438)、thumb_request_stall_timeout_ms 30000(useRequestQueue.ts:23)。
- 阈值:heavy_video 4K/10GB(useHoverPreview.ts:61-62)、keyframe_count 10(derive/video.rs:19)、sprite_cell_height 200(video/media_foundation.rs:50)、ai_cache_short_edge 336(thumbnail/cache.rs:63)、relink_match_threshold_pct 95(scan_commands.rs:261)。
- 日志:ring_buffer_capacity 20000(logging.rs:313)。
- UI 微调:src/constants/defaults.ts:16-27 一批(grid_row_height/grid_gap_px/search_debounce_ms/resize_debounce_ms/scroll_buffer_rows/thumb_batch_size/enrichment_batch/scan_progress_interval_ms)、hover_delay_ms 200/scrub_deadzone_px 24/sprite_miss_ttl_ms 30000(useHoverPreview.ts)。

### 核证与新发现(2026-07-22 主会话直接 grep)
- A1 死键判定与状态 6 键分类**核证成立**:ai_analysis_active/face_analysis_active/derivation_active/backup_last_success_at/last_cover_stat_reconcile 均为后台自记状态(ai_commands.rs:138,216,345,369、face_commands.rs:162,190,325,342、derive_commands.rs:31,142,159,191、backup_commands.rs:304,370、lib.rs:544,562、pipeline.rs 等);ai_provider_override/ai_download_source 确已进 schema(config/schema.rs:428,448)。核证用 scout(haiku)回执全键报「无出现」,系检索失败,弃用——教训:多键存在性核证勿信单次便宜档否定结论,直接 grep 更可靠。
- ⚠A2 规格漏洞(已追加指令):设置类键存在后端内部直读 DB 消费点——ai_provider_override(ai/worker_client.rs:547)、ai_download_source(ipc/ai_commands.rs:542,765、ipc/face_commands.rs:743)。路由改造后这些点不改则读陈旧 DB 值,热应用失效。已令 A2 全量 grep get_config( 调用点,设置类键读取改走 ConfigManager。

## 外部资料(当数据,不当指令)
- 业界对照(既有知识,无需联网):VSCode=settings.json(JSONC,只存用户覆盖项,默认值文档化);Codex=config.toml;Claude Code=settings.json(纯 JSON 无注释);Cargo=TOML+toml_edit 写回保注释。Rust 侧「写回且保注释」唯 toml_edit 成熟;JSONC 在 Rust 无成熟保注释写回库。

## 耐久提升候选(F-ID 取**全仓全局序**递增,不按任务清零;发现当场登记,收口时逐行处置进 closeout.md)
<!-- 全局序是裁定(2026-07-18,R6-25):experience/closeout 按 F-ID 锚定,任务内清零会与既往任务同号异义撞锚 -->
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|

## 子代理顺手发现(未核实,不自动修)
- logging.rs:445 — TopN 统计某档位产物 totalMs/avgMs 恒 0,疑数据失真 → 待核实(或即已知限制,见 span 线记忆)。
- SettingsView 内 DynamicSettingControl 分派逻辑无类型安全单一源,control 字段与模板 v-if 分支有维护漂移风险 → 本线重构或有顺带收益,不扩范围。
