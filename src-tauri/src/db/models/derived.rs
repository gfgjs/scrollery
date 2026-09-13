//! 派生结果(缩略图/文档) / 文本替换 / 版本快照 / 阅读器书签 / 应用统计域模型。

use serde::{Deserialize, Serialize};

// ── 应用程序统计 ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStats {
    pub total_items: i64,
    pub total_images: i64,
    pub total_videos: i64,
    pub total_audios: i64,
    pub total_documents: i64,
    pub total_favorited: i64,
    pub total_deleted: i64,
    pub total_live_photos: i64,
}

// ── Thumbnail result ─────────────────────────────────────────────────────────
/// 缩略图生成后返回的缩略图结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbResult {
    pub item_id: i64,
    pub thumb_status: i64,
    pub thumb_path: Option<String>,
    pub thumbhash: Option<Vec<u8>>,
    /// 生产该结果时读取到的源代次，仅供后端条件写使用。
    #[serde(skip)]
    pub source_revision: i64,
    /// 生产该结果时读取到的缓存键，仅供后端条件写使用。
    #[serde(skip)]
    pub cache_key: i64,
}

/// 等待前端渲染缩略图的文档（pdf/svg，§3.4 Lite 路径）。`abs_path` 由渲染器经 `convertFileSrc`
/// 包装以加载源文件。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingDocThumb {
    pub item_id: i64,
    pub abs_path: String,
    pub file_format: String,
}

/// 文本替换规则（§5.2）—— 角色扮演/人名映射的展示层查找替换，作用于单项/丛书组/全局，不改源文件。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplacementRule {
    pub id: i64,
    pub scope_kind: String, // 'item' | 'group' | 'global'
    pub scope_id: Option<i64>,
    pub find: String,
    pub replace: String,
    pub is_regex: bool,
    pub enabled: bool,
    pub sort_order: i64,
}

/// 文档版本快照（§5.3）—— 以原始件为基线的类 git 不可变快照树。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentVersion {
    pub id: i64,
    pub item_id: i64,
    pub parent_id: Option<i64>,
    pub label: Option<String>,
    pub storage: String, // 'appdata' | 'external'
    pub abs_path: String,
    pub source: String, // 'user' | 'ai-local' | 'ai-remote'
    pub note: Option<String>,
    pub content_hash: Option<String>,
    pub is_current: bool,
    pub created_at: i64,
}

/// 两个文档版本间的一条行级 diff（§5.3）。`tag`：equal/insert/delete。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffOp {
    pub tag: String, // "equal" | "insert" | "delete"
    pub value: String,
}

/// 一书中保存的一个阅读位置（§6.2, R4）。`locator` 现用 foliate CFI
///("cfi:<epubcfi>",与 reading_progress 同源);loc1 落地后可存 "loc1:<json>",表结构不变。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderBookmark {
    pub id: i64,
    pub locator: String,
    /// 展示标签：当前章名（回退全书百分比），供书签列表识别。
    pub label: String,
    /// 全书 progression 0..1：列表排序 + 百分比展示。
    pub fraction: f64,
    pub created_at: i64,
}

// ── Document meta（document_meta，Phase 2）────────────────────────────────────

/// 文档元数据行（`document_meta` 表）。PDF/epub 等的页数 + 子类型，文档 enrichment 完成后写入；
/// 消费在 Part3 文档派生（封面/进度条）与 Part5 阅读器（PDF 页 / epub 章节进度）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMeta {
    pub item_id: i64,
    /// 总页数（PDF 页数 / epub 章节数）；未读取为 None。
    pub page_count: Option<i64>,
    /// 文档子类型（pdf/svg/epub/office/text…，见 `utils::format::doc_subtype`）。
    pub doc_subtype: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::ThumbResult;

    #[test]
    fn thumb_production_snapshot_is_not_serialized() {
        let result = ThumbResult {
            item_id: 7,
            thumb_status: 1,
            thumb_path: Some("480/aa/thumb.webp".into()),
            thumbhash: None,
            source_revision: 9,
            cache_key: 123,
        };

        let json = serde_json::to_value(result).unwrap();
        assert_eq!(json["itemId"], 7);
        assert!(json.get("sourceRevision").is_none());
        assert!(json.get("cacheKey").is_none());
    }
}
