---
status: 快照
type: 工作记忆
line: 许可证迁移至 MPL-2.0
created: 2026-08-25
---

# 任务计划:许可证迁移至 MPL-2.0

## 目标
在不改变闭源 `scrollery-pro`、第三方依赖及既有 Apache-2.0 发布物授权的前提下，将后续公开 Community Edition 源码和分发合规声明从 Apache-2.0 迁移为标准 MPL-2.0，并使公开镜像、安装包源码告知、项目元数据及当前规范文档保持一致。

## 当前阶段
阶段 6:聚焦验证与收口

## 阶段

### 阶段 1:盘点与规划
- [x] 盘点根许可证、项目元数据、中英文 README、贡献与商标政策
- [x] 区分项目自身 Apache-2.0 声明与第三方依赖的 Apache-2.0 归属
- [x] 核对贡献历史、CLA 再许可授权、Copybara 公开投影和安装包资源现状
- [x] 对照 Mozilla、Apache 与 SPDX 官方资料确定最小合规路径
- **状态:** completed

### 阶段 2:权属与生效边界闸门
- [x] 用户确认当前唯一 Git 作者对应代码的再许可权利已由本人或相应权利主体掌握
- [x] 明确迁移自实施提交起向前生效；此前已按 Apache-2.0 获得的副本不撤回
- [x] 明确采用标准 `MPL-2.0`，不把 Exhibit B 的“不兼容次级许可证”声明附着到 Covered Software
- **状态:** completed

### 阶段 3:主许可证与机器可读元数据
- [x] 用 Mozilla 官方 MPL 2.0 原文替换根 `LICENSE`
- [x] 将 `package.json` 的项目许可证改为 `MPL-2.0`
- [x] 通过 npm 的 lockfile 更新流程仅更新 `package-lock.json` 根包许可证字段
- [x] 保持 Cargo 各内部、不可发布 package 的现状；本次不扩张为 Cargo 元数据治理
- **状态:** completed

### 阶段 4:对外声明、商标与分发合规
- [x] 定点更新 `README.md`、`README.zh-CN.md`、`CONTRIBUTING.md` 的项目许可证表述
- [x] 重写 `TRADEMARK.md` 中依赖 Apache-2.0 §6 的表述，保留品牌与代码授权边界
- [x] 增加简短源码获取告知，并随 Tauri 安装包携带 MPL 文本、源码地址告知和第三方 notices
- [x] 将 `scripts/generate-notice.mjs` 中“NOTICE 是 Apache §4(d) 载体”的项目级注释改为许可证中性表述
- **状态:** completed

### 阶段 5:规范文档与决策留痕
- [x] 新建许可证迁移决策 brief，记录范围、理由、生效点和旧版本不可追溯撤销
- [x] 修订当前正典 `Part0_总纲与产品定稿.md`、`Part8_商业化与分发.md` 与开发者手册
- [x] 回写 `docs/status/渠道与开源边界.md` 和 `docs/todo.md` 线索状态
- [x] 保留历史 decision/worklog/archive 中当时准确的 Apache-2.0 记录，不做全仓机械替换
- **状态:** completed

### 阶段 6:聚焦验证与收口
- [x] 校验 MPL 官方文本与 SPDX 标识准确
- [x] 搜索剩余项目级 Apache 声明并逐条分类，证明只剩第三方或历史上下文
- [x] 运行 `npm install --package-lock-only` 对应校验、NOTICE 新鲜度门和文档门禁
- [x] 对 Tauri 资源配置做聚焦构建/配置校验并生成 NSIS 安装包；安装包载荷闭包自检通过
- [x] 按 planning 技能完成蒸馏、状态回写并迁入 `docs/worklogs/`
- **状态:** completed

## 关键决策
<!-- 需收口提升的决策编 D-001 递增填「候选 ID」列;仅会话内有效的留空 -->
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| MPL 仅覆盖 Copybara 公开投影中的 Community Edition 第一方源码，闭源 `crates/scrollery-pro/**` 与 vendored/依赖代码沿用各自条款 | 防止根许可证误扩张闭源边界，也不篡改第三方授权 | D-001 |
| 采用标准 `MPL-2.0`，不把 Exhibit B 声明附着到 Covered Software | 用户指定的 SPDX 标识对应普通 MPL-2.0；保留默认次级许可证兼容性 | D-002 |
| 使用根 `LICENSE` 作为统一许可通知位置，不批量给数百个源码文件加头 | MPL Exhibit A 与 Mozilla FAQ 允许在不适合逐文件放置时使用接收者通常会查找的位置，符合本项目轻量原则 | D-003 |
| 安装包增加源码获取告知 | MPL 2.0 §3.2 要求可执行形式分发时告知 Covered Software 源码获取方式；当前 Tauri resources 未携带根许可证或源码告知 | D-004 |
| `NOTICE.md` 中第三方 Apache-2.0 条目不替换 | 它们描述依赖自身许可证，不是 Scrollery 主许可证 |  |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| 首轮并行盘点命令在 PowerShell 中以引号包裹可执行文件，触发 ParserError | 自动拼接 `"git" "status"` 等命令 | 改为原生 PowerShell 命令字符串后重跑，全部只读盘点成功 |
| 测试命令误带 Jest 风格的 `--runInBand`，Vitest 拒绝未知参数 | `npm test -- --runInBand` | 改跑项目定义的 `npm test`，139 个文件、1570 个测试通过 |
| `cargo metadata` 误带不支持的 `--workspace` 参数 | `cargo metadata --workspace --locked --offline --no-deps --format-version 1` | 改为 `cargo metadata --locked --offline --no-deps --format-version 1`，通过 |
| 文档门禁首轮缺少本任务对应的 line entity | 直接运行 `worklog-kit check` | 新增 `docs/lines/许可证迁移至-MPL-2.0.md` 后重跑通过 |
| Windows 调试程序占用 `target/debug/scrollery.exe` | `tauri build --no-bundle --debug --no-sign` 无法覆盖旧进程文件 | 不终止用户进程，改用 release NSIS 构建；安装包生成和载荷闭包校验通过 |
