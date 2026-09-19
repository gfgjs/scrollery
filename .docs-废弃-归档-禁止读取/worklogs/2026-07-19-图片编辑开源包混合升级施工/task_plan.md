---
status: 快照
type: 工作记忆
line: 图片编辑功能升级
created: 2026-07-19
---

# 任务计划:图片编辑开源包混合升级施工

## 目标
按 `docs/designs/2026-07-19-图片编辑开源包混合升级方案.md` 将未提交的 v1 几何编辑基线升级为受付费授权门控、使用降采样 sRGB 预览、支持拉直与三项调色且前后端结果可对拍的 v2 图片编辑功能，并完成设计规定的自动化与基准验证。

## 当前阶段
全部阶段已完成(2026-07-20 阶段 7 收官,证据见 findings.md)

## 阶段

### 阶段 1:基线盘点与准入 spike
- [x] 读取设计稿、文档契约、CI 门禁与当前工作树
- [x] 盘点 v1 编辑链、exotic 授权链、依赖与测试基线
- [x] 执行 P0-CM、imageproc 与前端裁剪双候选准入验证并记录裁决
- **状态:** completed

### 阶段 2:付费授权门接线
- [x] 复用 exotic entitlement，为编辑入口与保存 IPC 增加稳定门控；预览 IPC 创建时复用同一真门
- [x] 覆盖未授权、已授权、撤销授权的后端状态机，以及前端 fail-closed / 激活载荷回归测试
- **状态:** completed

### 阶段 3:E0 编辑预览资源链
- [x] 实现 orientation 烤入、CMS 转 sRGB、长边限制与编码返回
- [x] 前端改用 blob URL，确保编辑覆层不再解码全尺寸源
- [x] 覆盖尺寸上限、资源释放与坐标换算测试
- **状态:** completed

### 阶段 4:E1 裁剪交互升级
- [x] 对 vue-advanced-cropper 与 Cropper.js v2 按同表完成维护、审计、类型、受控状态与坐标语义 spike
- [x] 按设计决胜规则落地胜者；双败则保留并打磨自研拖拽层
- [x] 固化显示空间与源图归一化坐标换算测试，并以浏览器真实 DOM 冒烟验证选择框拖拽
- **状态:** completed

### 阶段 5:E2 拉直
- [x] 前后端实现闭区间 [-45°, 45°] 契约与最大内接矩形同源公式
- [x] Rust 接入 imageproc Bicubic fine rotate，0° 保持 v1 像素路径
- [x] UI 增加滑杆、三分网格与状态机测试
- **状态:** completed

### 阶段 6:E3 真色彩调色
- [x] 落地 CMS 选型、8/16-bit 位深保持与显式 sRGB 输出策略
- [x] 实现亮度、对比度、饱和度双端同源公式及共享黄金向量
- [x] 前端 canvas 预览接 rAF 节流，补状态机、重置与 hasChanges 测试
- **状态:** completed

### 阶段 7:整体验证与文档回写
- [x] 运行 rustfmt、clippy、Rust/前端聚焦测试及完整 CI 对应门禁
- [x] 验证依赖 feature、MSRV、许可/NOTICE、release 包体差值
- [x] 测量实际 Tauri 进程树 24/50/100MP 峰值；记录无法自动化的四平台手测缺口
- [x] 回写 `docs/todo.md` 与设计/状态文档中的实际裁决和实测证据
- **状态:** completed

## 关键决策
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 将现有未提交 v1 代码作为升级基线，不另起实现 | 设计稿明确 v1 阶段 1-4 已落地，保留已有工作并减少契约漂移 | |
| 新建独立 v2 三件套，不改写 v1 工作记忆的任务边界 | v1 与 v2 可分别追踪，且现有 v1 三件套已有在途记录 | |
| CMS 生产链采用 moxcms relative colorimetric，lcms2 仅作带 BPC 对拍参考 | moxcms 0.8.1 没有公开 BPC 开关；已测矩阵型 sRGB/Adobe RGB/Display P3 的码值差不超过 2 | D-701 |
| 裁剪层采用 Cropper.js 2.1.1，仅接管透明选择与手势 | 维护、类型与审计优于另一候选；不创建结果 canvas，保持 useImageEditor 为唯一图像状态源 | D-702 |
| 拉直内接矩形保持 fine rotate 前的原始宽高比 | 设计明确要求 Lightroom 同款语义；前后端共享 JSON 黄金向量与向下取整规则 | D-703 |
| 调色 CMS 走 moxcms f32 分条管线, 位深保持(8→8/16→16/f32→f32), 输出嵌显式 sRGB | 分条瞬态 O(条)使峰值≈双像素缓冲, 与 rotate90 已计量峰形同级, 不改 memory_budget 门; 像素已转 sRGB, 回写源 ICC 会让阅读器按错误空间解释 | D-705 |
| untagged(含 CMYK 边界)按 sRGB 直入且完全保持原变体, 灰度不升 RGB | saturate 矩阵行和恒为 1, 对灰像素严格恒等; 带 Gray ICC 的灰度经 CMS 升 RGB 是色彩转换固有结果 | D-704 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| 首次读取三件套出现乱码 | PowerShell 默认编码读取 UTF-8 | 后续显式使用 `Get-Content -Encoding utf8`，文件本身未损坏 |
| `cargo fmt -p scrollery -- <文件>` 把 `--` 后参数当 rustfmt 选项而非文件过滤, 重排全 crate, 连带 5 个 HEAD 本就 fmt-dirty 的无关文件(backup/core.rs 等) | 一次 | `git checkout --` 回滚 5 个无关文件; 既有 fmt drift 非本线引入不代修 |
| 阶段 7 用临时 `git worktree` 在 v1 基线提交 e3020c8 重建 release 二进制做包体差值对拍时,首次编译在 `lib.rs:820`(与图片编辑无关的既有窗口焦点→QoS 代码)先后报「bool 不能解引用」与「期望 bool 找到 &bool」两个互相矛盾的错误 | 怀疑源码错误,逐行核对 e3020c8/HEAD 该行字节级相同、tauri 版本锁定一致(2.11.4)、workspace 依赖清单亦相同,判定为随机编译失败 | 参照既有主机硬件不稳定记录(dev-machine-hw-instability),原样不改重跑一次即绿;确认属瞬时 flaky,非源码问题,不必修复 |