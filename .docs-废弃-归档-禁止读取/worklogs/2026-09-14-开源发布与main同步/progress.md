---
status: snapshot
type: working-memory
line: 渠道与开源边界
created: 2026-09-14
---

# 进度：开源发布与main同步

## 2026-09-14
- 已核对本地HEAD/工作树及既有同步配置；未推送。
- 计划使用三个白名单代理处理过滤/门禁、第三方、验证；主会话负责设计裁定、远端拓扑、最终审查与同步。
- 已发起commandcode High门禁实现、opencode-go Max第三方许可处理、jiyuanlvdong High验证执行。
- 远端私有main可快进到dev；公开main/staging同为7d5aa5a0；私有OSS_SYNC_DEPLOY_KEY已配置（只查名称，未读取值）。公开repo有写权。
- 原self-host Linux runner不存在（仅Windows runner offline），本机无Docker。批准A以单一规则Node快照工具替代不可用同步执行路径，保留公开bot/SQUASH/原锁验证语义。
- 从官方GitHub release下载gitleaks8.30.1 Windows x64，SHA256 d29144deff3a68aa93ced33dddf84b7fdc26070add4aa0f4513094c8332afc4e验证通过，工具仅位于target/oss-release-tools。
- 已克隆纯公开Git历史到target/oss-public-checkout（没有私有历史）；gitleaks git --all --redact=100扫描5提交、4.84MB退出0无命中。报告target/oss-release-tools/public-history-scan.json。新投影尚未扫描，不据此放行。

## 回顾
- 已交付：标准AGPL正文、限定Graphviz权限及商业说明；对应源包、推送前扫描和实际公开门禁；双仓main同步。
- 教训：本轮前半段把研究与同步框架展开过多；用户要求节省额度后停止原代理与重复验证，改为最小机械接线和单脚本发布。后续同类任务先做必要改动、一次精确快照和已有门禁。
- 证据边界：源码门禁通过不等于正式安装包/商业交付验收；并行测试问题仍留档，公开门禁使用串行。

## 用户收窄要求
- 用户指出工作被过度展开、消耗大量周额度，要求简化。主会话停止原同步/法律研究代理和重复测试，收回到两个明确的短收尾：已有法律材料机械接线；单个简短PowerShell快照脚本与现有workflow配置。不引入投影框架、manifest子命令或额外研究。
- 原Node投影方案尚未写入代码，取消；现行文档改指 scripts/publish-oss.ps1。既有源码验证结果复用，只核新增接线与最终公开快照。
- 法律材料收尾完成：NOTICE生成与--check、prepare:legal、bundle verifier自测/配置检查均通过；Graphviz/Viz.js/Expat三个源包纳入third-party/sources，SHA256与已核材料一致。官方安装包层未验证。
- 快照脚本与workflow已落文件，主审仅要求修复实际发布所需的脱敏、PowerShell参数传递、目标路径/仓库确认和字节保持问题；修后即开始精确提交扫描，不再扩展研究。
- 最终本地快照 `71d88d9e9ea8f69f3b8ae37b2b9f1ea6113c5573`，tree `dbc7ef578562dd90a18821265aaeed7b6cec90cb`；1583个文件的Git blob hash与mode逐一匹配源修订 `0b472be0` 减唯一排除表，内部路径均不在提交树。
- gitleaks对精确公开树（34.28MB）、固定提交元数据（394B）和6个公开提交历史（36.22MB）均无命中。初扫两处误报是shift+ArrowLeft快捷键与测试缓存整数，已按行标明真实语义；未屏蔽整个文件/规则。
- 私有dev/main已推送至 `0b472be0904c029147141db5bdf007e4faa8e9ee`；sync-oss运行34775864104已启动。公开main尚待门禁与提升，不提前宣称完成。
- 托管sync-oss成功；公开staging首修订fef5bbb0与本地快照tree完全相同。公开门禁34775899786发现日期区域格式导致的测试误判；89b54f9e修正精确期望行并让同步从sync-staging追加，52项局部测试通过。随后公开修订3df30586再次通过1583文件逐项对拍，公开前端1916测试/build、Rust workspace check及密钥扫描通过。
- 公开门禁34776090229的Rust串行测试发现两个夹具问题：Unix符号链接路径已由basic_fixture创建；缓存性能测试的真实墙钟越过1秒TTL。4719f7de仅修测试：移开原目录再构造逃逸链接；利用已有时间注入参数固定TTL逻辑时刻，真实IO测量照旧。局部缓存测试与rustfmt通过，Unix链接用例待公开Linux验证；RAW检查提前，避免后续测试失败掩盖其结果。私有main已推4719f7de，同步34776820853进行中。

## 许可裁定
- 主会话否决“EPL允许目标码分发+独立JS文件即可证明AGPL组合兼容”的代理结论；保留事实取证，要求限定处理。
- 用户已明确批准 graphviz-permission-draft.md 中的限定附加许可并保留功能。公开 ADDITIONAL-PERMISSION.md 已按草案落地；根README双语、COMMERCIAL、CONTRIBUTING、CLA说明、SOURCE与Tauri资源声明同步，根AGPL正文不改。
- 第三方代理继续处理Graphviz/Viz.js源码/构建配方、NOTICE及法律资源校验；最终公开扫描待定稿。
- Rust验证日志出现 fetch_io_profile_after_fix 失败与 concurrent_lifecycle_writers_restore_active_after_both_finish 长时间等待；由验证代理有界复现，尚未放行。
- 两项isolated运行均通过；代理随后重复并行全套仍有相同hang。主会话要求停止并行实验，严格改用 `cargo test --workspace --locked -- --test-threads=1` 完成串行证据，并检查独立RAW；不在本轮扩展业务源码根因研究。
- 主会话已回写Part0、Part6、Part6_3c现行同步说明、开发者手册、Spec09及docs隐私边界，最终需与A实现接口对拍。
- 文档门 `worklog-kit check` 本轮执行退出1，仍为26处既有问题和149条baseline豁免；本次修改/新增文件未报错。未修旁支文档问题。
- 原20分钟代理阶段到点后，A/B追加15分钟限定实施预算，C追加有界串行与RAW验证；均须在预算结束时回报实际完成与阻碍，禁止继续无边界探索。

## 验证进展（整合前）
- 前端日志显示类型检查无报错、168个测试文件/1916项测试通过、Vite生产构建完成；由验证代理核对退出码并出具最终记录。Rust workspace check已完成，串行测试仍在执行。
- 最终公开快照仍待过滤脚本与第三方法律材料定稿；当前测试不能替代精确公开树验证。
- 发布脚本采用固定bot作者、固定提交消息和源修订标记，公开Git历史单独克隆；禁止使用私有仓作为公开checkout的对象库。

## 最终结果
- 最终公开修订33c7f027102b9a46c88f8794d4228a4b5b1e060c，源码对应私有4719f7de。1583文件blob/mode再次逐项核验通过。
- 公开门禁34776879506、gitleaks和反向漂移告警全部success。main ruleset旧检查名对齐现行门禁，保留三项必需检查/禁止删除/禁止非快进且无bypass；公开main已非强制快进到上述修订。
