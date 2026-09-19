---
status: snapshot
type: working-memory
line: 渠道与开源边界
created: 2026-09-12
---

# 发现：主程序全开源迁移

## 需求与边界
用户授权开始实施上一轮方案。主程序第一方源码开放，不等于付费功能免费；第三方许可保持原样。

## 已核事实
- pro 的 DirectEntitlement 与主机 KeyringLicenseStore 重复；组合根使用 INTERNAL 块切换。
- pro 与 exotic-trust 的内置占位公钥同值但 key ID 不同；归一不能丢弃原 ID、用途、有效期或构建注入语义。
- 工作区已有大量其他任务改动；禁止整体重置、整体暂存或覆盖这些改动。
- 干净公开树的 Tauri build.rs 会校验未生成的 worker/DLL/legal 发行资源。本任务的 oss-gate 属源码检查，须以 job 局部 TAURI_CONFIG 清空 externalBin/resources（数组覆盖）；正式打包仍由原流程准备并校验资源。

## 耐久提升候选
| 候选 ID | 内容摘要 | 建议去向 |
|---|---|---|
| F-001 | 公钥配置归一必须保留 ID、用途和有效期；签发工具不因开源删除 | 现行插件规格与签发手册 |
| F-002 | 公开投影仅过滤内部文件，直接使用同一锁文件 | 同步规范与流水线规格 |
