---
status: snapshot
type: working-memory
line: 渠道与开源边界
created: 2026-09-14
---

# 进度：AGPL 与商业双重授权

## 2026-09-14
- 已检查原工作区并执行 git diff --check、暂存区 diff --check，均通过。
- 原有源码/测试/文档共29文件提交为 `7666104b`，未运行原业务改动测试，不声称业务验收通过。
- `.research-tmp/` 为原有未跟踪抓取材料/日志，保留本地；不纳入任何本任务提交。
- 已读取 planning 技能及 docs/README.md 文档治理约定；采用现行 ASCII frontmatter 枚举。
- 后续：委派三个互不重叠的修改范围，主会话终审与收口。
- 已委派白名单High代理：commandcode处理根法律文件，opencode-go处理元数据/法律资源/CLA工作流，jiyuanlvdong处理现行规范与状态；主会话独立核验迁移边界和第三方兼容风险。
- 局部搜索首次使用PowerShell中的路径通配参数触发rg路径错误，已通过明确现有文件完成目标证据读取；不据该失败声称全量搜索通过。
- B代理完成元数据与法律资源：package及lock根许可、12个Rust包、Tauri资源表及bundle检查加入COMMERCIAL.md；第三方依赖与Cargo锁未改。
- B代理验证：JSON解析、node --check、bundle内置selftest与实扫、NOTICE --check（rust460/npm164/vendored1/runtime2）、渠道配置扫描--no-dist、source-snapshot --selftest、cargo metadata --offline --locked --no-deps均通过。新COMMERCIAL文件落地前资源门曾正确报告缺项，文件落地后通过。
- 没有完整构建、Cargo编译/业务测试、真实安装包拆验；target/release/bundle不存在。
- B确认仓库无CLA工作流或签署记录；外部CLA-assistant配置及新版重签不能在本地验证，贡献指南采用合并前人工核验当前版同意，状态保留后续验收。
- 主会话终审要求修正初稿：商业说明限定AGPL取得的构建，不能免除官方商业版购买条件；内部修改不豁免适用网络条款；商业说明涵盖个人官方购买和企业OEM；SOURCE不能把仓库入口声明成已验当前发行源码可得，不把对应源码局限于第一方文件。
- A完成终审修正：标准AGPLv3正文来自GNU官网，与原下载换行归一后完全一致，SHA-256为0D96A4FF68AD6D4B6F1F30F713B18D5184912BA8DD389F86AA7710DB079ABCB0；COMMERCIAL中英明确两种商业范围与生效方式；CLA1.1(2026-09-14)面向有效同意，不追溯改变旧签署。
- SOURCE终审保留实际仓库入口，将对应源码可得性作为发行者责任，不声称当前公开仓已有所有发行版本源码；NAS修改版AGPL网络义务与第一方替代商业许可分清。
- 主会话已核验根法律文本、manifest/lock差异、资源验证器及关键规范段落；验证器最后仅注释订正以区分项目COMMERCIAL交付要求与许可证义务，不重复测试。
- C完成17处内部文档修订及新决策存证，历史许可结论加取代横幅；主会话补齐商业EULA须有效接受并满足条款的生效表述，状态表维持5项真实待办。
- 新建未入库工作记忆经绝对路径边界检查，用PowerShell Move-Item移入docs/worklogs/2026-09-14-AGPL与商业双重授权；不存在历史已提交目录可供git mv保留，未删除任何旧任务。
- 归档后worklog-kit check退出1：26处强制违反+149条baseline豁免，与代理基线相同，报错均为其它文件，本次closeout及修改文件未报错；worklog-kit index退出0。
- 最终git diff --check通过；NOTICE.md、third-party、src/vendor、public与Cargo.lock相对7666104b均无改动；LICENSE实测SHA-256与代理记录一致。
- 本次仅本地提交，不推送/同步公开镜像；原有及本次法律原文核验临时文件均留在未跟踪.research-tmp/。

## 回顾
- 亮点：先用独立提交保存既有UI与调查改动，再让法律声明、发布元数据、内部规范并行实施；终审聚焦许可适用范围而非重复业务测试。
- 教训：不能把AGPL商业自由写成所有官方构建免费，也不能将标准许可文本替换误写为第三方组合合规已完成。
- 意外：CLA签署机制实际不在仓库中；既有LibRaw/Graphviz许可组合在切AGPL后需单列发行核验，仍并入原有5项待办而非扩大本次实现。
