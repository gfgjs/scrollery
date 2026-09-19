---
id: 2026-07-02-Part6_3c_Copybara同步配置草稿
status: active
type: design
line: Part6 ③c 同步管线
created: 2026-07-02
last-verified: 2026-09-14
---

# Part6 ③c —— 私有源码到公开快照（现行说明）

> 2026-09-14 改用 PowerShell 已提交树快照，替代已失去 Docker/Linux 自托管执行条件的 Copybara。保留本文路径以维持历史引用；旧命令不再是发布入口。实际同步证据见 [渠道与开源边界状态](../status/渠道与开源边界.md)。

## 1. 现行拓扑

私有仓 `gfgjs/scrollery-private` 是 canonical 源；公开仓 `gfgjs/scrollery` 只接收从指定已提交修订生成的快照。公开提交只继承公开历史，不复制私有历史、原作者身份或原提交说明。

```text
私有 dev → 验证 → 私有 main
                    ↓ 共享过滤规则导出已提交树
             扫描拟公开内容与提交元数据
                    ↓ 固定 bot 快照提交、核对树一致
公开 sync-staging → oss-gate → 全绿后快进公开 main
```

代码与锁文件逐字节保留；不做源码剥离、lock 重写或功能裁剪。未跟踪文件不参与导出。

## 2. 单一发布入口

- `.github/workflows/sync-oss.yml` 在私有 main push 或手动触发时运行，使用 GitHub 托管 Ubuntu。
- `scripts/publish-oss.ps1` 是本地与 CI 共用的投影入口及过滤规则来源。只读取指定已提交修订；本地执行使用同一流程，不另写复制/删文件方案。
- 过滤内部 `docs/`、代理配置、工作记忆配置和模板、本地工具缓存、研究临时目录、仅内部治理使用的脚本/workflow，以及同步配置自身。保留应用构建、测试和法律材料所需文件。
- 公开 checkout 只能来自公开仓；不得从私有仓 clone、共享 objects/alternates 或把私有分支推入其中。
- 公开提交作者与提交者固定为 `Scrollery Sync Bot <sync-bot@users.noreply.github.com>`，使用固定说明和源修订标记。
- 推送目标仅 `sync-staging`；不强推，不在同步步骤直接更新公开 main。内部文档修改导致公开树不变时按正常无变更处理。

## 3. 推送前扫描

扫描发生在任何公开分支 push 之前。公开暂存分支同样是公开内容，不能依赖其事后检查阻止泄漏。

使用固定版本 gitleaks；下载资产须验证官方 SHA256。本地可提供已验证的扫描器。缺失扫描器、校验失败或扫描命中均停止发布。报告必须脱敏，不记录秘密原文。

扫描并核验拟公开文件和提交元数据；提交后核对实际 Git 树与已扫描投影一致。不得扫描一个目录再推送另一个未经核对的目录。

## 4. 公开门禁与提升

公开 `oss-gate.yml` 使用托管 runner，覆盖前端类型检查、测试和构建，Rust workspace 原锁检查与测试、独立 RAW 源码检查，以及公开历史密钥扫描。

Rust 源码检查通过 job 局部 `TAURI_CONFIG` 清空 `bundle.externalBin` / `bundle.resources`，不会伪造安装包验收。正式发行资源仍由原打包和法律材料检查流程验收。

同一公开提交的门禁通过后，维护者才快进公开 main。失败须先定位，不绕过失败检查，也不以私有工作树的通过记录代替最终公开投影证据。

## 5. 凭据与运行条件

私有读取使用 checkout 的仓内权限；公开写入使用私有仓的 `OSS_SYNC_DEPLOY_KEY`，该 key 仅允许写公开仓。日志和工件不包含私钥；临时凭据在执行结束时清理。

2026-09-14 只读检查时，私有仓仅有离线 Windows runner，没有 Linux runner；本机与 WSL 均无 Docker。若托管环境因额度或平台条件不可用，维护者可在本机运行同一脚本与门禁流程，保留扫描/提交树核验记录后使用已有公开仓写入权限。不得恢复一条绕过扫描的旧发布路径。

公开仓不挂自托管 runner。`drift-alarm` 检查 bot 身份；仓库保护规则是否配置应以远端实际状态为准，不能把文档中的预期当成已启用保护。

2026-09-14 已核对并对齐 main ruleset 的必需检查：`OSS 投影构建 gate`、`gitleaks 密钥扫描`、`反向漂移告警`。旧 `OSS strip-tree gate` 名称已替换为现行同一职责门禁；禁止删除、禁止非快进和零绕过主体规则保留。以后更名 job 须同步检查 ruleset，单查传统 branch protection API 不能代表全部保护规则。

关联：[Part0 §10](Part0_总纲与产品定稿.md)、[开发者项目手册](../runbooks/2026-07-06-开发者项目手册.md)、[Spec09](../spec/Spec09_插件平台与exotic.md)。
