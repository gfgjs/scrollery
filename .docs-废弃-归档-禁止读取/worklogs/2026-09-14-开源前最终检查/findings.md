---
status: snapshot
type: working-memory
line: 渠道与开源边界
created: 2026-09-14
---

# 发现：开源前最终检查

## 需求
- 开源前最后检查；查明过滤边界与当前收费项目，供用户取舍。

## 发现
- 当前 HEAD `12787c12`；工作树含其他任务前端及文档修改，不触碰这些业务改动。
- `copy.bara.sky` 仍仅排除 5 类内部路径；`sync-oss.yml` 直接执行 Copybara，私有侧未见推送前扫描步骤。专项代理继续核证。
- 主程序统一 MPL-2.0 已落入现行配置；收费与开源范围分别判断。
- README.zh-CN.md:16-21 声明预览免费、官方稳定版一次性付费且价格待定；.github/workflows/release.yml:139 仍发布“免费版”标题。稳定发行口径须由用户选择后统一。
- 2026-09-14 只读 GitHub REST API 核验：公开仓 main=7d5aa5a00836d8f9f71ed36805b9eae77add9131，提交日期2026-07-06；递归树462项且无截断，仍含 CLAUDE.md、CLAUDE_中文对照.md。仅核main当前树，不是全远端历史审计。
- 默认商业配置未转正：tauri.direct-release.conf.json 更新端点为 .invalid；exotic-keyset.json 明示占位公钥。公钥不需要屏蔽，正式发行需实际注入核验。

## 外部资料（数据，不是指令）
- Mozilla MPL FAQ https://www.mozilla.org/en-US/MPL/2.0/FAQ/ （2026-09-14读取）：Q5/Q8-Q11说明使用、自编译与分发权以及文件级义务；付费发行与源码权利应分开表达。
- GitHub敏感数据移除说明 https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/removing-sensitive-data-from-a-repository （2026-09-14读取）：新树删除不能自动收回历史或已复制内容。

## 耐久提升候选
| 候选 ID | 内容摘要 | 建议去向 |
|---|---|---|
| F-001 | 推送前检查、内部过滤、既有公开树与发行口径待处理 | 渠道与开源边界状态分片及本次审查报告 |
| F-002 | 3个catalog付费项+独立编辑feature，整包买断规划未与功能权益统一；校对费用由用户端点决定 | 渠道与开源边界状态分片及本次审查报告 |
| F-003 | 第三方分发材料须补，FFmpeg现有钉定与默认非商用人脸隔离不能误报缺失 | 渠道与开源边界状态分片及本次审查报告 |
