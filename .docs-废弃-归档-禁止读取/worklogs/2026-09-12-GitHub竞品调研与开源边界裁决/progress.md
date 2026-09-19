---
status: 快照
type: 工作记忆
line: 渠道与开源边界
created: 2026-09-12
---

# 进度日志:GitHub 竞品调研与开源边界裁决

## 会话:2026-09-12
- 做了:
  - 读产品侧权威文档:README、`docs/refactor_2026/Part0_总纲与产品定稿.md`(§2 定位/§3 竞品/§4 功能矩阵/§10 开源策略)、`docs/decisions/2026-07-06-决策brief-渠道·开源边界·face校验.md`、`docs/decisions/2026-08-25-决策brief-许可证迁移至-MPL-2.0.md`、`copy.bara.sky`、`docs/todo.md`、`docs/status/渠道与开源边界.md`。
  - 建立本任务三件套。
  - 派出 5 路后台子代理并行调研:①开源自托管媒体管理 ②开源桌面 DAM ③Rust/Tauri 同赛道 ④商业素材管理器定价 ⑤开源商业模式与许可证据。
- 验证:`curl -s https://api.github.com/repos/immich-app/immich` 网络可达(HTTP 200,返回 JSON)。
- 遗留:等待子代理报告 → 主会话交叉复核关键数字 → 综合与裁决。

## 会话:2026-09-12(续)

- 做了:
  - 主会话直查 GitHub API 20+ 个项目硬数据(星/许可/语言/push);撞 60/h 限流后用 web/raw 兜底。
  - **发现直接竞品 Lap**(`julyx10/lap`):2,260★ GPL-3.0 Vue/Tauri,push 2026-09-11,v0.3.1,README 覆盖本地 AI 搜索/人脸/RAW/编辑/去重/100k+。
  - 读 Lap `scripts/download_models.sh` 取证:CLIP=`openai/clip-vit-base-patch32`(英文中心),人脸=`deepghs/insightface` buffalo_s(`det_500m.onnx`/`w600k_mbf.onnx`)。
  - 读 InsightFace README 原文取证:代码 MIT 无限制,但**模型 non-commercial research purposes only**。
  - 读本仓代码核实开源边界:`scrollery-pro` 仅 ~150 行(keyring + 调开源 `exotic-trust` 验签 + 公钥 include_str!),无算法无私钥;`free-stub` 开源且可一行 fork 绕过;AI/人脸**当前无 entitlement 门**;`release.yml` 只出免费版;无移动端 gen 工程。
  - 查公开镜像 `gfgjs/scrollery`:1★、0 fork、最后 push 2026-07-06(GitHub 许可字段仍 Apache-2.0)。
  - 产出决策 brief 初稿 `docs/decisions/2026-09-12-决策brief-GitHub竞品与开源边界.md`。
- 子代理:5 路首派中 1 路成功(C Rust/Tauri 同赛道)、1 路成功(A 开源自托管)、3 路失败(GitHub 限流/抓取被拒);重派 B/D/E;B、D 在跑,E 再次失败后以 90 行上限重派。
- 遗留:等 B(桌面 DAM + Lap 深读)、D(商业闭源定价)、E(许可案例证据)落地 → 回填 brief §2.4/§4.3 → 收口三件套 + 回写 status/todo。

## 回顾(收口时填)
- 亮点:
  - **主会话直查 GitHub API + 读竞品源码取证**,比社区口碑/二手评测可靠得多。两处决定性证据都来自原始文件:Lap 的 `scripts/download_models.sh`(证明其语义模型是英文 CLIP)与 InsightFace README 原文(证明其模型非商用),这两条直接把「中文语义搜索」和「商用安全人脸」从纸面宣称变成可辩护优势。
  - **读本仓代码核实开源边界**,发现「部分开源」实际只藏约 150 行、且不含算法与私钥,把讨论从「开源多少」拉回「护城河到底是什么」。
  - 子代理分路 + 主会话兜底:三路失败两次后不再重派同一路线,改由主会话自行完成关键取证,避免无限重试。
- 教训:
  - **GitHub 无认证 API 60 req/h 且出口 IP 共享**,多子代理并行直查会互相撞限流;下次同类任务应"一个取数者 + 其余走 web"或先算配额。
  - 任务型子代理容易被"取不到数就重试"拖死;给它们的指令应包含「失败即跳过并标注」的硬约束。
  - 内部文档(`Part0 §10.3`「stub 无可绕过路径」)存在与代码不符的宣称;凡要写进对外结论的能力声明,必须回到代码验证。
- 意外:
  - **直接竞品 Lap 两个月从 1.3k★ 涨到 2.26k★**,且已发布、已公证、已有 9 语言文档;本产品公开镜像却停在 1★ / 停滞 2 个月。
  - 「部分开源 vs 完全开源」的前提被现实改写:AI/人脸/编辑管线**早已开源**,闭源层只剩验签胶水;真正的分界不在源码。
  - Lap 的旗舰卖点(InsightFace 人脸 + 50+ 语言搜索)在源码层站不住:模型非商用、语义模型是英文 CLIP。
