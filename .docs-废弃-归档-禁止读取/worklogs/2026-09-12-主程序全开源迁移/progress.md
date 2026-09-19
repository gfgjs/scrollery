---
status: snapshot
type: working-memory
line: 渠道与开源边界
created: 2026-09-12
---

# 进度：主程序全开源迁移

## 会话：2026-09-12
- 已确认已拍板范围、读取项目规则与现状，开始并行实施。
- 现状：代码尚有 pro/free-stub；Part0 已记录原则，技术迁移待完成。
- 验证：本轮尚未运行。
- 隔离：本任务独立工作记忆；执行者仅编辑分配写集，不提交、不推送。
- 起始 HEAD：`2118ff52026a453bf8ac69dd58c380c46d7c480c`，分支 `dev`，起始暂存区为空。
- 已修改文件基线备份位于 `C:/Users/gf/AppData/Local/Temp/scrollery-oss-247cc6b8fa6d4797b0ce2386cd266d88`；共享文件提交前须区分原改动与本任务增量。
- 执行代理：授权 `01a09346-7e3e-7300-89ba-9295e95744d0`；同步 `01a09346-7eeb-7553-a26f-9d7d66154f68`；声明/规范 `01a09346-7fdc-7f92-aa3a-b6fef3e1284e`。
- 本机有 Node/Rust/Python；未发现 Java/Copybara/Docker/gitleaks 可执行文件，WSL 亦未发现。公开投影将先在临时目录静态核对文件过滤与源码/锁文件一致性，再执行独立构建；真实 Copybara/远端 Actions 不冒称已跑。
- 同步批次完成：Copybara 仅保留五项原有内部文件过滤；公开门禁直接原锁构建/测试，签发工具与私钥排除保留。rename-gate、自测、YAML/JSON解析与相关diff检查通过。
- 主会话静态核验：Copybara AST语法、五项排除、空 transformations、SQUASH 模式均通过（不冒充真实Copybara运行）。七个前端文件只清理过期边界注释。
- 授权实施完成：删除 pro/free-stub，direct 唯一实现为 KeyringLicenseStore；默认 keyset 保留两组历史 ID；Cargo.lock 仅减少 18 行，无第三方版本变化。
- 聚焦验证：trust 27 + plugin-api 3；exotic 193 通过、2 个既有 ignored；msstore/steam 两渠道编译通过；全部第一方 Cargo 元数据解析为 MPL-2.0。
- 集中验证由授权代理串行执行；同步代理用临时 index 从 HEAD 加本任务写集准备独立公开投影，排除其他未提交工作，待主工作区 Cargo 完成后构建。
- 主审补正残留授权注释（含 state.rs 两处；该文件原有其他改动，提交仅取本任务注释增量），并要求公开声明保留未来独立商业组件另定许可、官方付费仍为计划态。
- 集中检查：cargo fmt / check --workspace --locked / clippy --workspace --locked -- -D warnings 均通过。并行 workspace test 在宿主测试阶段停止产生进展，执行代理在两次 CPU 读数间隔两分多钟不变后终止其进程树；同一宿主测试二进制以单线程运行 1336 通过、8 ignored，28.56 秒。并行根因未定，不宣称该次全量命令通过。
- NOTICE --check 通过（Rust 460 / npm 164 / vendored 1 / runtime 2）；本任务对 NOTICE 零增量，原有他人改动保留不提交。
- 文档声明/规格/手册完成；links-only 与 index check 通过。全局 check 仍有 21 处既有强制违反与 149 条 baseline 豁免，本轮修改的文档未新增失败。
- 独立投影由 HEAD 加本任务写集构成，1560 文件逐字节一致，5 类内部文件与两废弃 crate 均不存在。公开投影验证采用串行 harness 以完成整套测试；实际 Copybara/远端 CI 未执行。
- 干净投影首次 check/test 均因缺少打包资源 raw-worker sidecar 在 build.rs 退出 101，测试未开始。定位为既有发行资源前置被本地残留产物掩盖；公开源码门增 job 局部 TAURI_CONFIG 空 externalBin/resources，正式打包配置不变，不生成占位 exe。按该真实门禁环境再次验证。

## 回顾
本任务已完成。用户要求停止反复审查，沿用已完成验证直接收尾。

## 最终交付
- 独立公开投影在源码检查配置下 check 通过；workspace 串行测试（含 doc-tests）1494 通过、12 ignored、0 失败。
- 实际 Copybara、Linux 远端 CI、密钥扫描和安装包发布未执行；本轮只提交本地源码与文档。
- 全局文档门仍为同一批 21 处既有问题，本任务未新增；链接与索引检查已通过。
- 精确暂存本任务变更，共享 state/todo/worklogs 索引仅提交本任务增量，其他工作区改动保留。
