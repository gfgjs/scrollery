# 更新日志(Changelog)

本文件记录 Scrollery 面向用户的显著变更。格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/),版本号遵循 [SemVer](https://semver.org/lang/zh-CN/)。

> 发布流程约定(oss-release,Part7-T17):打 `vX.Y.Z` tag **之前**,须把 Unreleased 中对应条目
> 移入新的 `## [X.Y.Z] - YYYY-MM-DD` 小节——release 工作流从本文件提取该小节作为发布说明,
> **缺小节或小节为空即发布失败**(与「tag=版本锚一致性」同为硬门,倒逼发布纪律)。
> 公开仓提交历史为 squash 同步提交,不适用 conventional-commits 自动生成;本文件在私有
> canonical 仓维护、随同步进入公开镜像,是发布说明的单一事实源。

## [Unreleased]

> 首个版本累积中。发布前把以下条目移入 `## [0.1.0] - YYYY-MM-DD` 小节(见文首发布流程约定)。

### 变更

- **移除读屏/无障碍标注**:删除 ARIA/role/alt/live region 等读屏语义与减少动态效果适配;保留键盘操作、焦点陷阱与 inert 焦点隔离。
- **产品更名 Scrollery**(原工作代号 picasa-next,2026-07-06):应用标识符改为 `com.scrollery.app`,凭据库服务名改为 `scrollery`,数据库文件更名 `scrollery.db`。**内测用户升级路径**:卸载旧版 → 安装新版 → 重新激活 license(凭原激活码)、重录校对 API key / 存储后端密码(如曾配置);旧数据目录 `%APPDATA%/com.picasanext.app` 可手动删除,图库文件本体不受影响,缩略图 / AI 缓存自动重建。

### 新增

- **画廊浏览**:图片 / 视频 / 文档 / 音频统一媒体网格;两阶段扫描(秒级出图 + 后台 EXIF/XMP/Live Photo 补全);后端两端对齐 / 宫格布局 + 大库分段虚拟滚动(自研逻辑滚动条)。
- **缩略图管线**:WIC GPU 解码(Windows,含 HEIC/HEIF/AVIF)+ image-rs 跨平台回退;派生产物原子落盘;ThumbHash 模糊占位。
- **AI 语义搜索**:Chinese-CLIP 嵌入(ONNX Runtime / DirectML,运行于独立 ai-worker 子进程)+ 常驻 f16 向量缓存 + rayon 余弦打分。
- **人脸识别**:检测 / 嵌入 / 增量聚类 / 命名与合并。
- **资产组织**:收藏夹、收藏 / 评分 / 色标、文件夹树、批量移动 / 复制 / 删除(回收站)。
- **冷门格式插件平台**:签名 registry + 插件商店(安装 / 升级 / 修复 / 激活 / 卸载),首发 PSD;渠道物理门控(direct / MsStore / Steam 骨架)。
- **离线卷 UX**:离线灰显 + 角标、已知卷面板(改名 / 忘记)、重连自动恢复。
- **国际化**:简体中文 / 英文。
