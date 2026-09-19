---
id: 2026-07-17-designs-难项方案
status: active
type: design
line: 批量15项问题清单
created: 2026-07-17
---

# 难项设计稿:#7 迁移识别 / #9 去重缩略图 / #13 文档缩略图叠字

> **2026-07-17 用户裁决后状态**(本行为准,下方各节正文已按裁决更新):
> - **#7 = 方案 A,已施工落地**(commit a66d01a)。D-A 结案。
> - **#9 = 暂不做**(D-B 未拍板 → content_hash 地基不动;本项无独立立项价值)。结案 no-go。
> - **#13 = 另起独立线**,三件套见 `docs/planning/2026-07-17-文档缩略图叠加标题与章节/`,
>   D-C/D-D 随该线走,不在本线裁。
>
> 证据行号见同目录 findings.md。

## #7 文件夹迁移识别(D→C 盘免重扫免重生成)

**关键地面事实(已核实)**:缩略图 cache_key = xxh3(rel_path/file_name|mtime)(utils/hash.rs:29-40),
**不含盘符/绝对路径**。整根迁移后相对结构与 mtime 不变 → thumbnails/ai_thumbs/face_thumbs/sprites
全部产物文件名不变,可原样复用。数据库侧 directories.rel_path 也相对 scan_roots.path。
即:**唯一需要改的是 scan_roots.path 一行 + volumes 重绑**。

### 方案 A:手动「重链接根路径」——**用户已裁,已施工落地(a66d01a)**

落地形态(与本稿原设计的偏差已在下方「施工期地面事实更正」列明):
1. UI:文件夹区根节点右键菜单「文件夹已迁移…」(判据 `parentKey === null`)→ 选新路径 →
   二次确认 → 按稳定 code 分流三种话术 → 成功后**前端**补发一次增量重扫兜底。
2. 后端 `relink_scan_root(root_id, new_path)`:
   - 校验新路径是目录 → 查 `scan_roots.path` UNIQUE 冲突 → 抽样 100 项在新路径下 stat
     比对 **size+mtime**,命中率 <95% 即拒(空根放行)。抽样在**读池**做(含 N 次文件 IO,
     不可持写锁)。
   - `q::update_scan_root_path` + `PlatformVolumeResolver` 重探卷绑定 + `bump_data_version`
     + `tree_snapshots.invalidate_root`;新根补 `asset_protocol_scope().allow_directory`。
3. 失败路径:三个稳定 code `relink_not_a_dir` / `relink_path_taken` / `relink_mismatch`,
   保持原状不落库。
- 零 schema 改动、零缩略图重生成。6 个新测试(错误码契约 1 / DAO 2 / 命令层纯函数 3)。

#### 施工期地面事实更正(本稿原文有三处基于旧信息的错误,已按实码修正)
| 本稿原说法 | 实际(摸底核实) | 后果 |
|---|---|---|
| 「error.rs:117 **预留的** InvalidMove 可启用」 | InvalidMove **非预留**,已被 `ipc/file_ops_commands.rs` 8 处占用(675/680/685/693/711/853/858/864),语义是「文件夹移动非法」 | 复用会把两类无关失败挤进同一 IPC `code`,前端无法分流 → 改为新增专用 `AppError::Relink { code, message }`(同 Exotic/Reveal/Preview 稳定子码姿态) |
| 「复用 upsert Unchanged 判据(scan.rs:807-811),按 size+file_mtime 比对」 | Unchanged 判据在 **scan.rs:793,只比 `file_mtime`,size 不参与**;807-811 是 Unchanged 分支内的 UPDATE 参数行,不是判据 | 不复用:该函数吃 `&Connection`、要求先有 directories 行、Unchanged 分支仍可能写库。改为自写纯比较器,**且刻意保留 size+mtime 双比**(职责不同:upsert 问「文件变了吗」mtime 够;relink 问「同一棵树吗」要更强指纹,而 size 在同一次 metadata 里白送) |
| (未提) | `scan_roots.path` 有 **UNIQUE** 约束;且无 `update_scan_root_path` DAO | 需新增 DAO,并在命令层先查重给可读错误,否则 UPDATE 撞出裸 `AppError::Db` |

> 另两条落地约束(本稿原文未覆盖):
> - **后端自己触发不了兜底重扫**:`start_scan` 签名要 `Channel<ScanChannelPayload>`,进度
>   通道只有前端造得出 → 兜底重扫由前端在 relink 成功后补发(零后端重构)。
> - **新根必须补 asset scope 授权**:旧路径的 `allow_directory` 对新路径无效,漏了则迁移后
>   图片经 convertFileSrc 全部加载失败。
> - **卷绑定必须重探**:D→C 换了物理卷,不重绑则 volume_id 仍指旧卷,旧卷离线时缺失检测
>   会把整批本地文件误判 missing(C5 Piece1)。

### 方案 B:自动识别(大,依赖内容身份地基)
- 同物理卷换盘符:volumes.stable_id(卷 GUID)+ volume_relative_path 已可复认——现有机制。
- 跨物理盘迁移:无内容 hash 时只能启发式(目录名+文件计数+抽样 size/mtime 指纹)。
  可靠方案压在 §5.2 三步走(content_hash 一等公民)上,与 #9 同地基。
- **红线**:T13 rescan/relink 有意 defer(docs/completed.md:251),勿自动开工。

### 决策记录
- **D-A:已裁 = 做方案 A**(2026-07-17)。文案「文件夹已迁移…」,抽样 100 / 阈值 95%
  (阈值不取 100%:迁移后用户很可能顺手改过几张图,或同步盘占位落地致 mtime 抖动——
  见 `UpsertOutcome::SuspectChanged` 注释;5% 容差远严于错树的命中率,后者通常个位数)。
- **D-B:未拍板**。方案 B(自动识别)仍压在 §5.2 content_hash 地基上,**未开工**
  (T13 rescan/relink 有意 defer,docs/completed.md:251,红线:勿自动开工)。
  方案 A 已覆盖迁移主场景,B 的增量价值 = 免去「用户手动指新路径」这一步。

## #9 重复图只生成一张缩略图(评估结论)——**已裁:暂不做,结案 no-go**

> **2026-07-17 用户裁决:暂不做。** 理由与本节评估一致:本项无独立立项价值,收益全部
> 押在 §5.2 content_hash 地基上,而 D-B 未拍板 → 地基不动 → 本项无从做起。
> 下方评估正文保留作**未来重启本项时的现成账**(要做什么、成本几何、先量什么),
> 不是待办。重启的前置条件只有一个:D-B 拍板做 content_hash 地基。

**能否实现**:能,但不是小改。现状无全库内容 hash(content_hash 列存在但仅 suspect 变更时计算,
初次入库 NULL);缩略图键是 per-item 路径键非 per-content 键;同图两目录 = 两套全部派生产物
(缩略图+AI 嵌入+人脸,§5.2 明载)。

**要做的事**:① 全库 hash 回填流水线(建议 blake3——hash.rs:89 自注 sha2 是离线降级);
② 派生键改锚 content_hash(触及 cache.rs 全部路径助手、generator 缓存命中、LRU/GC 的
16-hex 命名护栏 cache.rs:455-471);③ 重复组「代表项」选择与删除时的所有权转移。

**性能账**:
- 一次性成本:500k 文件全量读+hash。blake3 在 NVMe 上 ~数 GB/s,瓶颈是盘读;
  按平均 5MB/张 ≈ 2.5TB 读,NVMe 顺序化后数小时量级,一次性、可后台分批。
- 持续收益:每张重复图省一次 解码+缩放+编码(远重于 hash);重复率高的库(相册导出/备份混合)
  收益显著,重复率低则近零。
- 建议先量重复率再定:`SELECT file_size, COUNT(*) c FROM media_items GROUP BY file_size HAVING c>1`
  粗估上界,决定值不值得。

**结论**:与 #7 方案 B 同地基,单独为 #9 立项不划算;若 D-B 拍板做 content_hash,
#9 作为其第二收益顺路落地。**已裁定 no-go(2026-07-17),不再等 D-B —— D-B 若日后拍板,
本项随之复活;D-B 不动则本项恒不做。**

## #13 文档缩略图叠加 文件名/内嵌标题/章节 —— **已移出本线,另起独立线**

> **2026-07-17 用户裁决:新建独立三件套,在新会话单独接续做。**
>
> **接续入口:`docs/planning/2026-07-17-文档缩略图叠加标题与章节/`**(task_plan / findings /
> progress)。本节原设计稿正文(现状摸底、零/一/二期分期、D-C/D-D)已**整体迁入该线的
> findings.md 与 task_plan.md**,是那条线的起点,不在此处维护——避免同一份方案两处漂移。
>
> 本线对 #13 的责任到此为止:摸底已做、方案已成形、线已建、指针已留。D-C/D-D 随新线裁。
