---
status: 快照
type: working-memory
line: 视频封面关键帧提取性能审查
created: 2026-07-17
---

# 任务计划:视频封面关键帧提取性能审查

## 目标
摸清视频封面/关键帧(雪碧图)提取流水线的性能地形,回答「是否用了 GPU」,产出分档优化方案供用户裁决;审查阶段不动代码。

## 当前阶段
阶段 5:真机 GUI 验收(⏸;代码/门禁/对拍已收)

## 阶段

### 阶段 1:代码摸底
- [x] 定位调用链:derive/pipeline.rs → derive/video.rs → video/media_foundation.rs
- [x] 确认解码路径(CPU/GPU)、并发预算、调度让步逻辑、任务取序
- **状态:** done

### 阶段 2:瓶颈定位与方案分档
- [x] 逐环节成本画像(reader open ×3 / 全分辨率 RGB32 转换 / 多次全尺寸拷贝 / 调度暂停)
- [x] 方案分档:T1 CPU 廉价改 / T2 GPU(D3D manager+DXVA) / T3 调度感知
- **状态:** done

### 阶段 3:产出报告待裁决
- [x] 报告交付用户,用户裁决:**全做**(T1+T2+T3)
- **状态:** done

### 阶段 4:按裁决施工
- [x] 4a 基准:bench 样本(H.264 1080p/4K、HEVC 4K、rotate=90 校验片)+ 改前基线数据
- [x] 4b T1:advanced processing+输出尺寸协商 / 反选音频 / 砍 probe open / 免 clone+向量化拷贝
- [x] 4c T2:D3D11 device manager + DXVA 硬解,GPU 槽位限流 + 软解回退(hw available=true 实证)
- [x] 4d T3:交互窗口涓流 1 worker(扫描/缩略图仍全让步;in-flight 计数)
- [x] 4e 收尾:改后对拍(HEVC 4K 全套 6.3×)+ rotation PASS + fmt/clippy/test 全绿 + 提交
- **状态:** done

### 阶段 5:真机 GUI 验收
- [ ] 大库扫描后封面出现速度 + 交互涓流手感 + 竖拍手机视频方向抽查
- **状态:** pending(⏸GUI)

## 关键决策
<!-- 需收口提升的决策编 D-001 递增填「候选 ID」列;仅会话内有效的留空 -->
| 决策 | 理由 | 候选 ID |
|------|------|---------|
| 审查阶段不动代码 | 用户问的是「如何优化+是否用 GPU」,交付物是评估;方案分档待裁决 | |
| 用户裁决:T1+T2+T3 全做 | 2026-07-17 用户「全做」 | |
| T1c 只砍 probe open,**不**合并 cover+keyframes 共用 reader | 合并会破坏「封面全局先行」取序与按 kind 断点续传模型;省 1 次 open 相对 10 帧解码收益小 | D-001 |
| T2 GPU 失败回退软解,GPU 槽位 try_acquire 满则走软解不阻塞 | 硬解会话有限;满载时软解并行推进吞吐,优于排队 | D-002 |
| 旋转定案:输出类型 rotation+FRAME_SIZE 双属性钉死,旋转权威唯一归 CPU apply_rotation | XVP 自动转正拦不住(SetRotation 无效)、属性不可判;双属性钉死是实测唯一稳修(F-005) | D-003 |
| 诊断单测 diag_pipeline_env 留仓(#[ignore]+env 门控) | XVP/几何类问题复发时 13s 一轮拿地面真相;不进常规测试路径 | D-004 |

## 错误账
| 错误 | 尝试 | 解法 |
|------|------|------|
| rot90 双旋转(480x360 横幅) | ①解码坐标系请求+恒 CPU 旋 | XVP 自动转正叠加所致,失败 |
| 同上 | ②读输出类型 rotation 属性判残余 | 属性不清零(实测缺失),失败 |
| 同上 | ③SetRotation(ROTATION_NONE) 拦 XVP | 拦不住,失败;两连败后停手换 diag 单测观测 |
| 内容不转但信箱化(四角黑边) | diag 实证输出尺寸被 MF 默认成旋后宽高 | 输出类型 rotation+FRAME_SIZE 双属性恒钉死(D-003),PASS |
| bench exe 路径 not recognized | src-tauri/target/... | target 在 workspace 根(profile 迁根所致) |
