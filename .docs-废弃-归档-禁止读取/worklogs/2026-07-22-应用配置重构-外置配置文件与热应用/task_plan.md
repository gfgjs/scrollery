---
status: 快照
type: working-memory
line: 应用配置重构-外置配置文件与热应用
created: 2026-07-22
---

# 任务计划:应用配置重构-外置配置文件与热应用

## 目标
所有应用设置(含当前设置页未暴露的可调参数)迁入单一带注释的外置配置文件(格式参照 VSCode/Codex/Claude Code 业界方案,倾向 TOML/JSONC 待摸底后定);设置页提供「用外部编辑器打开配置文件」入口;外部编辑保存后实时热应用;设置页 UI 与配置文件双向同步且不丢注释。

## 当前阶段
全部完成,余:用户裁决清单+GUI 真机验收

## 阶段

### 阶段 1:摸底 — complete(3 路 Explore 回执合并入 findings.md「发现」节;后端 55 键/7 IPC/热应用副作用清单、前端双 store 无事件通道、散落硬编码 ~30 项)

### 阶段 2:方案设计 — complete(决策见下表;边缘项入「待用户裁决清单」)

### 阶段 3:后端施工 — complete(A1:config 模块 5 文件+59 键 schema+25 测,死键 8/状态 18 排除,thumb_quality 判死键核证成立;A2:三路路由+apply_setting_effects 抽取+watcher 挂载+open_config_file/get_config_status+内部直读 DB 全量改走 ConfigManager,cargo --lib 882 测绿 clippy 零警告;均未 commit,待审)

### 阶段 4:前端施工 — complete(B:useConfigFile+双 store refreshFromBackend+设置页入口/横幅+中英文案,9 文件,eslint/vue-tsc 0、vitest 1345 绿;未 commit,待审)

### 阶段 5:散落项接线 — complete(C:12 个 advanced 键全部真接线,后端运行时键改读 ConfigManager、前端阈值/debounce 键经 get_startup_config 增量字段下发,无摆设键;段1 b216dcc + 段2 5b0b925)

### 阶段 6:测试/门禁/复核/收尾 — complete(opus 深审 3P1+5P2 全修复、增量复核 8/8 过、三 commit 收账;段1 b216dcc / 段2 5b0b925)

## 待用户裁决清单(拿不准项,回来逐条裁)
1. 键分类边缘项:layout_mode/group_by/sort_within_group/sidebar_width 按「布局记忆=状态」留 DB 未进文件——若认为属用户设置可翻案。
2. 散落深水区是否开放为配置:db_read_pool_size、journal_size_limit(64MB WAL)、edit_peak_bytes_ceiling(1.5GB)、background_heavy_limiter_permits——默认本线不动。
3. 模板形态已拍板「全键注释态」(见 D-c03),若偏好「全实值写出」可翻案(代价:升级默认值时用户文件钉死旧值)。
4. localStorage 调试 3 键(bucketSpacer/renderMode/perfProbe)是否收编进 [advanced]——默认收编 renderMode,其余两个留 localStorage(纯开发探针)。
5. 现 IPC 表面(get/set_app_config)保持兼容不改名——若想借机换新契约(如 settings.get/set 命名空间化)需另立批次。

## 关键决策
<!-- 需收口提升的决策编 D-001 递增填「候选 ID」列;仅会话内有效的留空 -->
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| D-c01 格式=TOML,文件=app_data_dir/config.toml,写回用 toml_edit | 「UI 写回不丢注释」是硬需求,Rust 侧唯 toml_edit 成熟(Cargo 同款);Codex 同选型;前端零新依赖(解析全在后端) | D-711 |
| D-c02 设置/状态分离:用户意图进文件;应用记忆(schema_version/last_*/sidebar_width/first_launch/pinned_settings/ai_gpu_name/exotic_paused/layout_mode/group_by/sort_within_group)留 DB | 状态进文件=噪音且外部编辑无意义;VSCode settings vs workspaceState 同分层 | D-712 |
| D-c03 模板=全键注释态(每键中文说明+`# key = 默认值`),用户取消注释即覆盖;UI 改动写实值 | 只存覆盖项,默认值升级不被旧文件钉死;VSCode 理念 | D-713 |
| D-c04 文件=设置类键唯一真源;DB 旧值一次性迁移(仅非默认值写实值),迁后该类键不再读 DB、不删旧行 | 单一真源防双写漂移;不删=回滚安全 | D-714 |
| D-c05 热应用=notify watcher(监听父目录过滤文件名,兼容编辑器 rename 保存)+500ms debounce+内容指纹防自写回环;语法错→整体不应用+报事件保旧值;单键类型错→跳该键收警告,余键照常应用 | VSCode 同行为;编辑器原子保存会断 inode,必须监听目录 | D-715 |
| D-c06 IPC 表面兼容:保留 get/set_app_config/get_startup_config 名义与语义,后端路由改存储;新增 open_config_file/get_config_status 命令+config-file-changed{keys}/config-file-error{line,message} 事件 | 前端改动面最小;契约可后置演化 | D-716 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| scout(haiku)死键核证回执全键报「无出现」,与 A1 结论矛盾且自相矛盾(状态键明明在用) | 1 次 | 主会话直接 grep 一次了结:A1 正确、scout 检索失败;顺带揪出 A2 规格漏洞(内部直读 DB 的设置键) |
| 追加指令误发给 scout(收件人 ID 搞混) | 1 次 | 已叫停 scout(只读代理无写害),重发正确收件人 A2 |