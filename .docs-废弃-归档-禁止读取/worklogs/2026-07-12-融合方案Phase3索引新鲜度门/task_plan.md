---
status: snapshot
type: working-memory
line: 融合方案Phase3索引新鲜度门
created: 2026-07-12
---

# 任务计划:融合方案 Phase 3 索引新鲜度门

## 目标
用户拍板执行 Phase 3:消除 docs/README 与 worklogs README 手工索引的漂移风险,并同步更新 ~/.codex skill 副本;设计以地面事实为准(元数据是否支撑生成式)落地并回写方案。

## 当前阶段
已收口(2026-07-12):三阶段全部完成,融合方案四阶段就此全交付

## 阶段

### 阶段 1:设计裁决与副本同步
- [x] 核实「从 frontmatter 重生成索引」的前提是否成立(登记表各列是否元数据可得)——不成立,见 findings
- [x] ~/.codex skill 副本复验:已同步(--check exit 0 + SHA-256 双端一致),无需安装
- **状态:** completed

### 阶段 2:索引门实现
- [x] tools/check_docs_index.mjs(目录表↔实际目录/字母登记表不变量/worklogs 登记双向一致)+ --selftest 7 例全绿
- [x] ci.yml 挂两步(真仓门+自检);check_docs/check_docs_index/canonical 全绿
- **状态:** completed

### 阶段 3:回写与收口
- [x] 融合方案 Phase 3 行按红线改判回写(生成式被否→不变量门)+施工注更新+§5-#9 债项闭环;todo 治理增补⑤ 更新
- [x] 本三件套走 closeout 门收口
- **状态:** completed

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 生成式索引重建被否,Phase 3 落地为不变量新鲜度门 | 登记表 立项/权威文档 列与 worklogs 摘要列系人工判断,frontmatter 不含;生成式须发明新字段且把人工蒸馏降为机器倾倒 | D-001 |
| ~/.codex 副本不做安装动作 | --check 与 SHA-256 双证已同步 | |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
