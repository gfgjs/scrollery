# C5 剩余（Part2 T2）落地 scoping：卷插拔监听 + 原生卷 ID

> 📦 **已归档(2026-07-10 文档治理)**:C5/Part2-T2 两片已于 2026-06-29 落地(Part2 §3 T1/T2 行有账,commit 530924c / ba4f452);本档为施工 scoping 存证。

> 状态：**两片均已落地（2026-06-29）**。前置 C5 Piece1/Piece2（volume_id 管道）已落地，缺失检测对新数据生效。
> - **Piece B 卷插拔监听（poll 轮询）✅ commit `ba4f452`**：15s 对账 + online↔offline 实时翻 + 正交铁律（5 单测），前端 `volumes:changed` 刷新徽标。
> - **Piece A 原生卷 GUID ✅ commit `530924c`**：Windows `PlatformVolumeResolver`（卷 GUID 抗盘符重映射，失败回退路径派生）。
> - 🔵 仍后续：mac DiskArbitration `DAVolumeUUID`；Windows `WM_DEVICECHANGE` 即时唤醒（仅提前唤醒轮询，逻辑不变）。
> 真机验证项：拔插 U 盘 ≤15s 离线/恢复徽标；换盘符后同卷复认。

## 0. 背景与现状

缺失检测的「卷在线集」目前靠**扫描时**的 `PathProber` 判定；卷身份由 `PathVolumeResolver` 路径派生（`path:C:`，不抗盘符重映射）。两个缺口：
- **无实时插拔感知**：拔盘后画廊不会立刻翻「离线」，要等下次扫描。
- **卷身份不稳定**：盘符变（E:→F:）后路径派生 ID 变，同一物理卷被认成新卷。

现有可复用基础设施（已实现，Piece1/2 + V10 DAO）：
- `volumes` 表 + DAO：`upsert_volume` / `get_volume_by_stable_id` / `list_volumes` / `set_volume_online`（在线翻转，保留 last_seen/mount_path）/ `bulk_set_availability(volume_id, from, to)`（整盘 availability 切换，**仅合法整盘状态切换**）/ `delete_volume`。
- `VolumeOnlineCheck` trait + `PathProber`（路径可访问性在线判定，可注入 mock）。
- `VolumeResolver` trait + `PathVolumeResolver`（路径派生身份，数据模型按「卷根」并卷，**前向兼容**原生 GUID）。

## 1. Piece B — 卷插拔监听（poll 轮询，跨平台，优先做）

### 1.1 目标
后台线程实时维护 `volumes.is_online` 与 `media_items.availability(online↔offline)`，不必等扫描；启动对账一次。

### 1.2 设计
- **机制**：专用后台线程，每 ~15s 轮询一次（plan §3.1.2-3 既定「15s 轮询兜底」即作为 v1 主机制——跨平台、零原生 FFI、最简）。启动时立即对账一次。
- **每轮逻辑**（短锁、绝不跨 sleep 持锁）：
  1. `list_volumes` 取所有已知卷（连接快照后立即释放锁）。
  2. 对每卷用 `VolumeOnlineCheck::is_online(last_mount_path)` 判定当前在线态。
  3. 与 DB `is_online` 比对，仅在**变化**时写：
     - online→offline：`set_volume_online(stable_id, false)` + `bulk_set_availability(volume_id, 'online'→'offline')`。
     - offline→online：`set_volume_online(stable_id, true)` + `bulk_set_availability(volume_id, 'offline'→'online')`。
  4. 有任一卷状态变化 → emit Tauri 事件 `volumes:changed`，前端据此刷新画廊（`invalidateLayout` + reload）。
- **正交性铁律**：监听**只动 availability(online↔offline)**，**绝不碰 is_deleted、绝不动 'missing'**。`missing` 是扫描差集的结论（文件级），`offline` 是卷级，两者不互转——离线卷重连后由扫描恢复 missing，监听只管 online/offline。
- **锁纪律**（项目约定）：std::sync::Mutex 不跨 `.await`；本线程是同步线程（非 async 命令），每次取锁→查/写→立即释放，sleep 在锁外。

### 1.3 落点
- 新建 `scanner/volume_watch.rs`：`VolumeWatcher`（持有 `Arc<AppState>` 或所需句柄 + `Arc<dyn VolumeOnlineCheck>`），`run_once(conn) -> Vec<VolumeChange>`（可单测）+ `spawn(app, state)` 起线程。
- `lib.rs` setup：app 启动后 `VolumeWatcher::spawn(...)`。
- 关停：随进程退出（daemon 线程）；或持 `Arc<AtomicBool>` 停止标志（可选）。

### 1.4 验收
- **单测**（`run_once` 纯逻辑，注入 mock online-check + in-memory DB）：
  - 卷由在线变离线 → 该卷 media 'online'→'offline'，is_deleted/missing 不动。
  - 卷由离线变在线 → 'offline'→'online'。
  - 状态未变 → 零写、零事件。
  - 'missing' 项不被监听改动（只 online↔offline）。
- **真机手动**（按 DoD「未自动覆盖」）：拔 U 盘 → ≤15s 画廊该盘项变灰「离线」；插回 → ≤15s 恢复。

### 1.5 风险
- 轮询粒度 15s（非实时）：v1 可接受；未来可叠加 Windows `WM_DEVICECHANGE` 即时触发（仅作「提前唤醒轮询」，逻辑不变）。
- 与扫描并发写 DB：均走 `db_writer` 同一把锁，短临界区，无死锁；availability 切换与扫描差集互不踩（一个卷级一个文件级）。

## 2. Piece A — 原生卷 GUID/UUID（抗盘符重映射，B 之后做）

### 2.1 目标
`stable_id` 从路径派生升级为真实卷标识，盘符变仍复认同一物理卷。

### 2.2 设计
- **依赖**：复用**已有** `windows` crate v0.58，仅新增 feature `Win32_Storage_FileSystem`（零新增外部依赖，符合 §51 既定模式）。
- **Windows 实现**（`#[cfg(windows)]` `WinVolumeResolver`）：
  - `GetVolumePathNameW(path)` → 卷挂载根（如 `E:\`）。
  - `GetVolumeNameForVolumeMountPointW(mount)` → `\\?\Volume{GUID}\`，取 GUID 作 `stable_id`。
  - `GetDriveTypeW(mount)` → `VolumeKind`（Removable/Fixed=Local/Remote=Network）。
  - **防御**：任一 API 失败 → 回退 `PathVolumeResolver`（绝不因解析失败而中断添加根；保守可用）。
- **非 Windows**：暂沿用 `PathVolumeResolver`（mac DiskArbitration `DAVolumeUUID` 留后续）。
- **接线**：`add_scan_root` 按 `#[cfg(windows)]` 选择 resolver；trait 不变，数据模型「卷根并卷」不变 → 原生 GUID 只是升级既有卷行 stable_id，**无合并迁移**。

### 2.3 验收
- 原生 API 路径无法纯单测（需真实卷）→ 按 DoD「未自动覆盖」：`cargo check` + 真机手动（拔插换盘符后同卷复认）。
- 可单测部分：GUID 字符串规整、失败回退到 PathVolumeResolver 的分支。

### 2.4 风险
- FFI `unsafe` + 宽字符缓冲：小心缓冲区长度 / NUL 终止 / UTF-16 转换。封装在单函数内、错误即回退。
- 仅 Windows 实测；mac/linux 走 path 派生（功能不退化，只是不抗重映射）。

## 3. 推荐顺序
**先 B（poll 监听）后 A（原生 GUID）**：
- B 跨平台、零 FFI、复用既有 DAO、即时可见价值（实时离线徽标）、可单测，风险最低。
- A 是 Windows FFI（虽零新依赖），improves 身份稳定性，但纯真机验证、风险略高，放后面。
- 两者数据模型解耦：监听按 mount_path/stable_id 工作，与身份方案无关 → 顺序不产生数据问题。
