---
status: 施工中
type: 工作记忆
line: 玻璃模式内容底面修复
created: 2026-09-06
---

# 进度日志:玻璃模式内容底面修复

## 会话:2026-09-06
- 做了:按方案 §5 变更清单全量施工——
  - glass.css:token(90% content fill)+ opt-in 清单(settings/collections/persons/plugin-store/doc 五根)+ settings-header/doc-toolbar 置透明 + 文件头接入手册;
  - 配置链 9 文件:schema.rs → config_commands.rs → uiStore.ts → uiScale.ts → settingsMap.ts → DynamicSettingControl.vue → SettingsView.vue → ipcFixtures.ts → 双语 i18n;
  - 测试:uiStore.spec +2 用例;glass.spec 契约随新规则更新(计划外,施工中发现);
  - 文档:同次修订 2026-08-24 上游方案四处被证伪表述;回写 status 分片 + todo.md。
- 验证:
  - `cargo test --workspace --locked`:1355 passed / 0 failed(含 schema 遍历式新键覆盖);
  - `npm run typecheck`:通过;
  - `npm test`:155 files / 1747 tests 全过(1745 + 新 2);
  - `npm run lint`:通过。
- 遗留:方案 §7 真机手动验收全部条目(用户执行);验收过后三件套迁 worklogs 收口。commit 未做(待用户确认或随下批提交;工作区另有主题重构未提交遗留,不混批)。

## 回顾(收口时填)
- 亮点:方案 §4.2 的「根面裸则加 fill」判据把三个待核视图的处置变成一次实查即可裁定;glass.spec 契约测试让 glass.css 规则形变当场被测试网接住。
- 教训:(待收口补)
- 意外:方案变更清单漏列 glass.spec.ts(既有契约测试)与 ipcFixtures.ts/SettingsView.vue(键同步面),实查补齐——配置键类施工的「全仓 grep 旧键名」比照清单走更可靠。
