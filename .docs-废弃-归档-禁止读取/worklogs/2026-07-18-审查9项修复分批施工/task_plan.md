---
status: 快照
type: working-memory
line: 审查9项修复分批施工
created: 2026-07-18
---

# 任务计划:审查9项修复分批施工

## 目标
外部 AI 审查报告(docs/reviews/2026-07-18-昨日与今日代码修改审查.md)F-01..F-09 全部 9 项已逐条核实属实;按批修复并补测试,分阶段 commit,无人值守推进。

## 当前阶段
阶段 5:收尾(施工全部完成,待真机 GUI 验收与用户收口指令)

## 阶段

### 阶段 1:批1 = F-01 + F-02 + F-06(run-generation 纪律 + reset 错误传播)
- [x] 抽 `RunTokenSlot`(generation + token 槽,begin/cancel/finish compare-and-clear),thumb_gen_token 与 derivation_token 迁入;ai/face 已有 generation,本批不动
- [x] pipeline.rs 收尾改 finish(generation);derivation_active 清 0 仅当本轮仍是当前轮
- [x] thumbnail_commands.rs 终态发布 + token 清槽仅当本轮仍是当前轮
- [x] derive_commands.rs reset 失败向 IPC 传播,不启动流水线(F-06)
- [x] RunTokenSlot 单元测试:旧轮 finish 晚于新轮 start、stop 后 finish、连续 begin(4 例)
- [x] cargo test 649 过 + clippy 0;commit `efec238`
- **状态:** done

### 阶段 2:批2 = F-03 + F-05 + F-08(relink 事务 + scope 时序 + 前端错误边界)
- [x] relink_scan_root:校验全过后再 allow_directory;四步 DB 写并入单 transaction;卷探测移出写锁;commit 后失效缓存
- [x] 顺检 add_scan_root 同款先授后验姿态——一并对齐(授权后置到落库成功后)
- [x] FoldersSection.vue relinkRoot 拆两个错误边界(重链接提交 / 后续重扫);i18n 新增 relinkRescanFailed
- [x] 测试:事务故障注入**未做**(queries 无注入 seam,记 gap);typecheck/eslint 绿;commit `0d1234a`
- **状态:** done

### 阶段 3:批3 = F-04(TXT 超长单行内存放大)
- [x] ~~text_index.rs 行内切分~~ **裁不做**(D-001):decoded-char→源字节映射在非 UTF-8 编码是新偏移风险面;characterization 测试钉住局限
- [x] BookReader.vue TXT 分支补 256K 强制 scrolled 护栏;阈值/谓词提升 syntheticBook.ts 单源(FORCE_SCROLLED_SECTION_CHARS/hasOversizedSection)
- [x] 测试:Rust single_giant_line_stays_one_chapter + vitest 护栏 5 例;commit `305d4b6`
- **状态:** done

### 阶段 4:批4 = F-07 + F-09(MF 流选择回滚 + 旋转写串行化)
- [x] media_foundation.rs select_video_only:反选失败早退(零副作用);单流选回失败回滚 ALL_STREAMS=true
- [x] mediaStore setViewRotation 迁新原语 utils/latestWrite.ts(按 key 串行 latest-write-wins + onError);失败不回滚乐观角度(会话内画面正确,重开回退持久值)
- [x] 测试:latestWrite 单测 5 例;MF 回滚需真源/COM mock **不可自动化**(记 gap);commit `8b0220f`
- **状态:** done

### 阶段 5:收尾
- [x] 全量测试:cargo test --workspace 731 过 0 败(基线 726+新增 5);vitest 1234 过(基线 1224+新增 10)
- [x] 审查报告顶部标注修复状态;docs/todo.md 登记(2026-07-18 增补条)
- [x] 三件套回写
- [ ] ⏸ 真机 GUI 验收:停止→立即重启派生/缩略图不互杀、重链接失败路径、巨型单行 TXT 开卷、快速连点旋转
- **状态:** in_progress(施工毕,余真机验收 + 用户收口)

### 阶段 6:批5 = F-025(ai/face 令牌槽统一迁 RunTokenSlot;2026-07-19 用户批开工)
- [x] ai_analysis_token / face_analysis_token 字段升 RunTokenSlot;删共享 analysis_token_gen(代次改各槽独立——finish 只与本槽代次比较,无跨槽比较,无观察面变更)
- [x] 六 wrapper 改薄委托,签名不变;finish_* 返回 bool,ai/face 完成回调**刻意丢弃**(终态副作用门控保持 `!token.is_cancelled()`,不引入 thumb 的 finish-bool 门控姿态)
- [x] 三处直接字段访问 `.lock().is_some()` → `.is_running()`(ai_status / face_status / recluster_faces 守卫);GPU F5 互斥不涉
- [x] 验证:cargo test --workspace 731 过 0 败(持平)+ clippy 0 + fmt 0;commit `0bf5ebb`
- **状态:** done

## 关键决策
<!-- 需收口提升的决策编 D-001 递增填「候选 ID」列;仅会话内有效的留空 -->
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 抽 RunTokenSlot 只迁 thumb+derive,ai/face 暂留原样 | ai/face 已有 generation 语义正确;本批控爆炸半径,统一迁移留后续 | |
| finish() 语义:槽空(被 cancel 清)= 本轮仍持终态发布权 | 用户显式 stop 后旧轮仍须发 cancelled 终态;仅被更新一轮占用时才抑制 | |
| TXT 后端不做行内切分,前端超限章强制 scrolled 兜底 | decoded-char→源字节映射在非 UTF-8 编码是新的偏移正确性风险面;MD 同款护栏有实测背书;characterization 钉住,未来实现时测试应反转 | D-001 |
| relink 旧根 asset 授权不撤销 | scope 无逐条撤销 API,多根可共享父目录前缀,误撤连坐;进程重启按当前根重授自然收敛 | |
| F-09 写失败不回滚乐观角度 | 无持久基线可回滚;当前画面仍正确,重开自然回退;仅留 console 证据 | |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|