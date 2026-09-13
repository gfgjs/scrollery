//! 收藏夹 / 媒体过滤器 / 视图描述符与选择契约域模型(T18 / T14.5)。

use serde::{Deserialize, Serialize};

use crate::error::AppError;

// ── 收藏夹 ────────────────────────────────────────────────────────────────────

/// 由 `albums` 表承载的收藏夹（§3.7）。
///
/// `kind='system'` → 播种的 4 个类型夹之一（image/video/audio/document）；成员虚拟（`media_type_filter` + is_favorited）。
/// `kind='user'` → 成员存 `album_items`，可跨类型混装。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub media_type_filter: Option<String>,
    pub icon: Option<String>,
    /// 卡片缩略图的封面项（最新成员）；由前端解析缩略图。
    pub cover_item_id: Option<i64>,
    pub item_count: i64,
    pub sort_order: i64,
}

// ── 媒体过滤器 ─────────────────────────────────────────────────────────────

/// PartialEq：重复镜头分支以 `filter == MediaFilter::default()` 判定「无成员级筛选」
///（2026-09-02 方案 §10.2——镜头固定全库，任何非默认字段必须显式拒绝而非静默忽略）。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MediaFilter {
    pub media_types: Option<Vec<String>>,
    /// 细分格式筛选（S 线 D-011）：规范化小写扩展名，维度内 OR、与 `media_types` 取 AND。
    ///
    /// `None` / 空 = 该维度不限（**不是**筛出零条）。存具体扩展名而非 UI 的 `group`
    ///(JPEG={jpg,jpeg})—— `file_format` 是文件的客观属性,`group` 只是显示概念,不落库也不入 API。
    pub file_formats: Option<Vec<String>>,
    pub live_photo_only: Option<bool>,
    pub favorited_only: Option<bool>,
    pub min_rating: Option<i64>,
    /// 颜色标签筛选：精确匹配某色档（1-7；0=未标）。与 min_rating 同为逐项小标量筛选（T16）。
    pub color_label: Option<i64>,
    pub date_range: Option<DateRange>,
    pub directory_id: Option<i64>,
    /// 过滤为某用户收藏夹（album_items）的成员。系统夹改用 `media_types` + `favorited_only`，无需 JOIN。
    pub album_id: Option<i64>,
    /// 过滤为包含归属此人物簇人脸的图像（F6 人物墙 → 某人物的照片）。前端与其它视图筛选互斥。
    pub person_id: Option<i64>,
    pub search_query: Option<String>,
    pub search_scope: Option<String>,
    pub ai_search: Option<bool>,
    pub ai_threshold: Option<f64>,
    pub trashed_only: Option<bool>,
    pub recent_only: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DateRange {
    pub from: i64,
    pub to: i64,
}

impl MediaFilter {
    /// 全部字段均为 None（= `MediaFilter::default()`，无任何筛选谓词）。重复镜头入口
    /// （compute_layout）据此判定「成员级筛选为空」（compute_layout 收到的是 MediaFilter
    /// 而非 GalleryFilter，不能复用 [`GalleryFilter::is_empty`]）。新增字段时必须同步进
    /// 此判断——漏掉一个就会让镜头静默忽略该谓词，方案 §10.2 的显式拒绝防线即失效。
    pub fn is_empty(&self) -> bool {
        self.media_types.is_none()
            && self.file_formats.is_none()
            && self.live_photo_only.is_none()
            && self.favorited_only.is_none()
            && self.min_rating.is_none()
            && self.color_label.is_none()
            && self.date_range.is_none()
            && self.directory_id.is_none()
            && self.album_id.is_none()
            && self.person_id.is_none()
            && self.search_query.is_none()
            && self.search_scope.is_none()
            && self.ai_search.is_none()
            && self.ai_threshold.is_none()
            && self.trashed_only.is_none()
            && self.recent_only.is_none()
    }
}

// ── 视图描述符与选择契约（T18 / T14.5）───────────────────────────────────────
//
// 面向 >100 万项库：选择不枚举 id，而是描述「哪个视图的全集，减去哪些排除项」，由后端按
// filter 在 SQL 层流式解析 —— 既不把百万 id 灌进前端内存，也不经 IPC 整包传 id。

/// 排序规格：决定 ORDER BY，与 layout 分组同源（对齐 uiStore.groupBy / sortWithinGroup）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SortSpec {
    pub group_by: String,          // "date" | "folder" | "none"
    pub sort_within_group: String, // "datetime" | "filename" | "similarity" ...
    pub sort_order: String,        // "asc" | "desc"
}

impl Default for SortSpec {
    fn default() -> Self {
        // 与画廊默认一致：按拍摄时间倒序、日期分组。
        Self {
            group_by: "date".into(),
            sort_within_group: "datetime".into(),
            sort_order: "desc".into(),
        }
    }
}

/// 视图集合来源（决定 FROM/JOIN 与基础谓词）。与 `GalleryFilter` 分工：scope 定**来源**，
/// filter 在来源上**再筛**。这些字段从 `MediaFilter` 剥离至此（D1），避免「同一语义两处可填、互相打架」。
/// ⚠️ serde 细节（R1-2 契约定形）：enum 级 `rename_all` 只改**变体名**，不改 struct 变体内的
/// 字段名——各携带字段的变体须自带 `rename_all`，前端才能以 camelCase（directoryId 等）构造。
/// 此前该路径无前端消费者，形状错配从未暴露；wire 格式已由 queries.rs 的 S1 锁测试钉死。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ViewScope {
    /// 全库（is_deleted=0）。「智能相册/系统夹」= All + GalleryFilter(media_types/favorited)，不单列 scope。
    All,
    /// 某目录递归子树（复用 query_layout_items 的 WITH RECURSIVE dir_tree）。
    #[serde(rename_all = "camelCase")]
    Directory { directory_id: i64 },
    /// 用户收藏夹（album_items 成员）。
    #[serde(rename_all = "camelCase")]
    Collection { album_id: i64 },
    /// 人脸簇视图（某人物的照片）。model_name 隔离（D2）v1 单模型下为 no-op，多模型随 Part4 T6 接入。
    #[serde(rename_all = "camelCase")]
    Person { person_id: i64 },
    /// 回收站（is_deleted=1）。
    Trash,
    /// CLIP 语义搜索：有序、非纯 SQL（v1 由 ai_search 既有路径承载，`view_to_sql` 不直接支持）。
    #[serde(rename_all = "camelCase")]
    SemanticSearch { query_embedding_id: i64, top_k: u32 },
}

/// 附加筛选（在 scope 选定来源上再筛），决定 WHERE 增量。**不含 scope 字段**（D1：scope 字段归 ViewScope）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GalleryFilter {
    pub media_types: Option<Vec<String>>,
    /// 细分格式筛选（S 线 D-011）。**必须随描述符走**：SelectAll 经 `to_media_filter()` 解析全集，
    /// 这里漏一个字段，批量收藏/评分/删除的目标集就与用户看到的画面漂移。
    pub file_formats: Option<Vec<String>>,
    pub live_photo_only: Option<bool>,
    pub favorited_only: Option<bool>,
    pub min_rating: Option<i64>,
    /// 颜色标签筛选：精确匹配某色档（1-7；0=未标）（T16）。
    pub color_label: Option<i64>,
    pub date_range: Option<DateRange>,
    pub search_query: Option<String>,
    pub search_scope: Option<String>,
    /// 「最近导入」智能相册（R1-2 补）：此前 GalleryFilter 无法表达 recent 视图，
    /// 该视图下的 SelectAll 描述符会静默丢失谓词、作用到错误集合。
    pub recent_only: Option<bool>,
}

impl GalleryFilter {
    /// 全部字段均为 None（无任何成员级筛选）。重复镜头 MVP 只接受空 filter
    ///（方案 §10.2）；新增筛选字段时必须同步进此判断——漏掉一个就会让镜头
    /// 静默忽略该谓词，被 [`DuplicateLensDescriptor::validate`] 拒绝的防线即失效。
    pub fn is_empty(&self) -> bool {
        self.media_types.is_none()
            && self.file_formats.is_none()
            && self.live_photo_only.is_none()
            && self.favorited_only.is_none()
            && self.min_rating.is_none()
            && self.color_label.is_none()
            && self.date_range.is_none()
            && self.search_query.is_none()
            && self.search_scope.is_none()
            && self.recent_only.is_none()
    }
}

/// 重复镜头模式（2026-09-02 主画廊重复项浏览方案 §10.2）。
/// wire: "groups" | "folders"。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DuplicateLensMode {
    Groups,
    Folders,
}

/// 镜头当前支持的排序契约版本。未来调整镜头排序语义时递增，
/// 使旧描述符被显式拒绝而非被新语义错误执行（方案 §10.2）。
pub const DUPLICATE_LENS_ORDERING_VERSION: u32 = 1;

/// 主画廊重复镜头描述符（方案 §10.2）。附加在 ViewDescriptor 上：
/// 缺失 = 普通画廊（wire 形状与既有完全一致）；存在 = 重复镜头视图。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateLensDescriptor {
    pub mode: DuplicateLensMode,
    pub show_unique_items: bool,
    pub ordering_version: u32,
}

impl DuplicateLensDescriptor {
    /// 方案 §10.2 MVP 校验：镜头只允许全库 scope + 空 filter；
    /// showUniqueItems 仅 folders 模式；orderingVersion 必须匹配当前契约版本。
    ///
    /// 依赖方向：models → crate::error 无环（error 只依赖 dedup/config/ai_core），
    /// 故直接返回 AppError，调用方零转换。
    pub fn validate(&self, scope: &ViewScope, filter: &GalleryFilter) -> Result<(), AppError> {
        if !matches!(scope, ViewScope::All) {
            return Err(AppError::DuplicateLensInvalid(
                "重复镜头只支持全库范围 | Duplicate lens only supports the whole-library scope"
                    .into(),
            ));
        }
        // 静默忽略成员级谓词 = 镜头画面与 SelectAll 解析的集合漂移（方案 §10.2 明令不接受），
        // 故非空 filter 一律显式拒绝，而非降级执行。
        if !filter.is_empty() {
            return Err(AppError::DuplicateLensInvalid(
                "重复镜头内不支持成员级筛选 | Duplicate lens does not support member-level filters"
                    .into(),
            ));
        }
        if self.mode == DuplicateLensMode::Groups && self.show_unique_items {
            return Err(AppError::DuplicateLensInvalid(
                "按重复组模式不支持显示独有项 | showUniqueItems is only allowed in folders mode"
                    .into(),
            ));
        }
        if self.ordering_version != DUPLICATE_LENS_ORDERING_VERSION {
            return Err(AppError::DuplicateLensInvalid(format!(
                "镜头排序契约版本不匹配（期望 {}，实得 {}） | Ordering version mismatch (expected {}, got {})",
                DUPLICATE_LENS_ORDERING_VERSION, self.ordering_version,
                DUPLICATE_LENS_ORDERING_VERSION, self.ordering_version
            )));
        }
        Ok(())
    }
}

/// 重复镜头下逐项分类桶（方案 §3.4）：duplicate=当前有效精确组成员；
/// unconfirmed=无法安全定案；unique=最近完整分析证明无副本。线上小写字符串。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DuplicateBucket {
    Duplicate,
    Unconfirmed,
    Unique,
}

// ── 重复组稳定 key（方案 §6.1）───────────────────────────────────────────────
//
// dedup IPC（旧 /duplicates 画廊）与主画廊镜头布局共用的组身份编码，避免两份同码
// 实现漂移。形状为 `{"digest":"<base64url 无填充>","size":<unit_size>}` 的 JSON——与
// 旧 dedup_commands::encode_group_key 逐字节同码，旧前端持有的 group_key 仍可解码。

/// 组 key 的 wire 形状（encode 与 dedup IPC 的 decode 共用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct GroupKeyWire {
    pub(crate) digest: String,
    pub(crate) size: i64,
}

/// 重复组稳定 key（方案 §6.1：内部稳定身份 + separator groupId，不暴露摘要原文）。
/// 序列化 `{String, i64}` 实际不可失败，返回 String 免去调用方 Result 传播——
/// 不可达的 Err 兜底为空串（不含用户数据，无真实失败源）。
pub fn encode_group_key(digest: &[u8], size: i64) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    serde_json::to_string(&GroupKeyWire {
        digest: URL_SAFE_NO_PAD.encode(digest),
        size,
    })
    .unwrap_or_default()
}

/// 不可变视图描述符：唯一确定「当前画廊视图全集 + 序」，是 `view_to_sql` 的输入、全选解析的依据。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewDescriptor {
    pub scope: ViewScope,
    pub filter: GalleryFilter,
    pub sort: SortSpec,
    /// 重复镜头（方案 §10.2）：None = 普通画廊。skip 确保旧 wire 形状逐字节不变。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_lens: Option<DuplicateLensDescriptor>,
    /// 与 `LayoutCache.layout_version` 对齐；解析时不一致即拒（`AppError::ViewStale`）。
    pub layout_version: u64,
}

impl ViewDescriptor {
    /// 把 scope + filter **lower 成既有 `MediaFilter`**，复用 `query_layout_items` 同一套 SQL builder
    ///(D1:单一事实源,不另起双套 WHERE,杜绝视图定义漂移)。
    pub fn to_media_filter(&self) -> MediaFilter {
        let mut mf = MediaFilter {
            media_types: self.filter.media_types.clone(),
            file_formats: self.filter.file_formats.clone(),
            live_photo_only: self.filter.live_photo_only,
            favorited_only: self.filter.favorited_only,
            min_rating: self.filter.min_rating,
            color_label: self.filter.color_label,
            date_range: self.filter.date_range.clone(),
            search_query: self.filter.search_query.clone(),
            search_scope: self.filter.search_scope.clone(),
            recent_only: self.filter.recent_only,
            ..Default::default()
        };
        // scope 决定 FROM/JOIN 与基础谓词，映射回 MediaFilter 的对应字段。
        match &self.scope {
            ViewScope::All => {}
            ViewScope::Directory { directory_id } => mf.directory_id = Some(*directory_id),
            ViewScope::Collection { album_id } => mf.album_id = Some(*album_id),
            ViewScope::Person { person_id } => mf.person_id = Some(*person_id),
            ViewScope::Trash => mf.trashed_only = Some(true),
            // SemanticSearch 在 view_to_sql 入口已被拦截，此分支仅为穷尽匹配。
            ViewScope::SemanticSearch { .. } => mf.ai_search = Some(true),
        }
        mf
    }
}

/// 选择 = 描述而非枚举（百万级不灌前端内存 / 不经 IPC 整包传 id）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SelectionDescriptor {
    /// 显式 id 列表（手选少量项）。上限校验见 `resolve_selection`。
    Explicit { ids: Vec<i64> },
    /// 全选某视图 − 排除集（Ctrl+A）。`excluded_ids` 通常远小于全集。
    /// `view` 经 `Box` 装箱：`ViewDescriptor` 远大于 `Explicit` 变体，避免枚举按最大变体撑大
    ///(clippy large_enum_variant)。serde 对 `Box<T>` 透明,前端 JSON 契约不变。
    /// 变体级 `rename_all` 使 `excluded_ids` 上线为 `excludedIds`（同 ViewScope 注意事项）。
    #[serde(rename_all = "camelCase")]
    SelectAll {
        view: Box<ViewDescriptor>,
        excluded_ids: Vec<i64>,
    },
}
