// src-tauri/src/ai/search.rs
//! 基于 CLIP 嵌入向量的内存余弦相似度搜索（C1）。
//!
//! Embeddings are loaded from SQLite ONCE into a resident, half-precision (f16)
//! contiguous buffer (`EmbeddingCache`) and reused across queries. Cosine similarity
//! is computed with rayon across all rows. This replaces the previous design that
//! re-read every embedding from SQLite (≈2GB at 1M items) on every single query.
//!
//! 嵌入向量从 SQLite **一次性**载入常驻的半精度（f16）连续缓冲区（`EmbeddingCache`）
//! 并跨查询复用；余弦相似度用 rayon 跨全部行并行计算。此设计取代了此前"每次查询都
//! 从 SQLite 重读全部嵌入向量（百万项约 2GB）"的实现。
//!
//! P1-3(2026-09-12):本文件负责「快照身份判定 + 打分」(模型名与维度整体比对,见
//! [`EmbeddingCache::matches`]);请求代次、缓存单飞装载与提交/清空/切模型的线性化在
//! [`crate::ai::search_control`]。结果集(ai_search_results)的读、写、擦也在这里,
//! 由 `ai_commands` 的语义搜索命令族经控制面调用——全仓只有这一条搜索主路径。

use half::f16;
use rayon::prelude::*;
use rusqlite::Connection;

use crate::error::{AppError, Result};

/// Resident, half-precision embedding store kept in `AppState`.
/// `data` is row-major: row `i` occupies `data[i*dim .. (i+1)*dim]`, paired with `ids[i]`.
///
/// 常驻于 `AppState` 的半精度嵌入向量存储。
/// `data` 行主序：第 `i` 行占 `data[i*dim .. (i+1)*dim]`，与 `ids[i]` 配对。
pub struct EmbeddingCache {
    pub model_name: String,
    pub ids: Vec<i64>,
    pub data: Vec<f16>,
    pub dim: usize,
}

impl EmbeddingCache {
    pub fn len(&self) -> usize {
        self.ids.len()
    }
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// 在阻塞线程逐行读库并构造常驻快照，不保留原始 f32 BLOB 全集。
    pub fn load(conn: &Connection, model_name: &str, dim: usize) -> Result<Self> {
        let count = crate::db::queries::count_embeddings_for_model(conn, model_name)? as usize;
        let mut cache = Self::with_capacity(model_name, dim, count);
        crate::db::queries::for_each_embedding(conn, model_name, |id, blob| {
            cache.push_blob(id, blob);
        })?;
        Ok(cache)
    }

    fn with_capacity(model_name: &str, dim: usize, count: usize) -> Self {
        Self {
            model_name: model_name.to_string(),
            ids: Vec::with_capacity(count),
            data: Vec::with_capacity(count * dim),
            dim,
        }
    }

    // 维度不同的旧向量继续跳过；借用仅持续当前行，转换后只保留 f16。
    fn push_blob(&mut self, id: i64, blob: &[u8]) {
        if blob.len() != self.dim * 4 {
            return;
        }
        self.ids.push(id);
        self.data.extend(
            blob.as_chunks::<4>()
                .0
                .iter()
                .map(|chunk| f16::from_f32(f32::from_le_bytes(*chunk))),
        );
    }

    /// 测试夹具与生产逐行装载共用相同转换与维度过滤。
    #[cfg(test)]
    pub fn pack(model_name: &str, dim: usize, raw: Vec<(i64, Vec<u8>)>) -> Self {
        let mut cache = Self::with_capacity(model_name, dim, raw.len());
        for (id, blob) in raw {
            cache.push_blob(id, &blob);
        }
        cache
    }

    /// 快照身份判定:模型名与维度必须整体一致才可复用。
    /// 只比模型名会在维度错配时按快照维度索引查询向量——同维静默算错,异维越界 panic。
    pub fn matches(&self, model_name: &str, dim: usize) -> bool {
        self.model_name == model_name && self.dim == dim
    }
}

/// 对常驻快照打分:单位向量的余弦 == 点积,rayon 跨全部行并行,降序取 Top-K。
///
/// 查询向量长度必须等于快照维度:不符返回 `Err`,不按快照维度索引查询向量
/// (越界 panic 会让前端只拿到笼统 JoinError,2026-07-10 审查 A7 的教训)。
pub fn score_top_k(
    cache: &EmbeddingCache,
    query_vec: &[f32],
    top_k: usize,
) -> Result<Vec<(i64, f32)>> {
    if query_vec.len() != cache.dim {
        return Err(AppError::Internal(format!(
            "查询向量维度 {} 与缓存身份 {}(dim {}) 不符(模型错配?)",
            query_vec.len(),
            cache.model_name,
            cache.dim
        )));
    }
    if cache.is_empty() || top_k == 0 {
        return Ok(Vec::new());
    }

    let dim = cache.dim;
    let mut scored: Vec<(i64, f32)> = cache
        .ids
        .par_iter()
        .enumerate()
        .map(|(i, &id)| {
            let row = &cache.data[i * dim..i * dim + dim];
            let mut dot = 0.0f32;
            for k in 0..dim {
                dot += query_vec[k] * row[k].to_f32();
            }
            (id, dot.clamp(-1.0, 1.0))
        })
        .collect();

    // NaN 无可用相似度，不能参与排序或写入结果表；±Infinity 已由 clamp 收敛。
    scored.retain(|(_, score)| score.is_finite());
    // 同分按 ID 固定边界，只对留下的 k 项排序，避免为未返回的结果做全量排序。
    let compare =
        |a: &(i64, f32), b: &(i64, f32)| b.1.partial_cmp(&a.1).unwrap().then_with(|| a.0.cmp(&b.0));
    if top_k < scored.len() {
        scored.select_nth_unstable_by(top_k, compare);
    }
    scored.truncate(top_k);
    scored.sort_unstable_by(compare);
    Ok(scored)
}

/// 擦除共享结果集(清空搜索/切模型/重建嵌入的落库动作)。
///
/// 调用方须在 [`crate::ai::search_control::SearchControl`] 的闸门内执行本函数,
/// 否则「撤销在途请求」与「擦表」之间会留出旧结果复活的窗口。
pub fn wipe_search_results(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM ai_search_results", [])
        .map_err(AppError::from)?;
    Ok(())
}

/// 用本轮打分结果整表替换共享结果集(事务;打分期间不持写锁)。
/// 空结果同样落在事务里:空库 = 合法空结果集,不是「没写过」。
pub fn replace_search_results(conn: &mut Connection, scored: &[(i64, f32)]) -> Result<usize> {
    let tx = conn.transaction().map_err(AppError::from)?;
    tx.execute("DELETE FROM ai_search_results", [])
        .map_err(AppError::from)?;
    {
        let mut stmt = tx
            .prepare("INSERT INTO ai_search_results (file_id, similarity) VALUES (?1, ?2)")
            .map_err(AppError::from)?;
        for (id, sim) in scored {
            stmt.execute(rusqlite::params![id, sim])
                .map_err(AppError::from)?;
        }
    }
    tx.commit().map_err(AppError::from)?;
    Ok(scored.len())
}

/// 两个预归一化单位向量之间的余弦相似度。
///
/// 对于单位向量：余弦相似度 = 点积。
#[inline]
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    // 维度不匹配 = 跨向量空间比较(如切换人脸模型后 128 维旧质心 vs 512 维新嵌入)。
    // 查询层已按 model_name 双过滤隔离(Part4-T6),此处是最后防线:返回 0.0(低于一切
    // 同人/搜索阈值 → 永不判同)而非 panic——原 debug_assert 会让 debug 构建整个进程
    // 炸掉,违背「模型切换不 panic」(T_t3);而 zip 截断在 release 下是静默算错,更糟。
    if a.len() != b.len() {
        tracing::warn!(
            "Embedding dimension mismatch {} vs {} — treated as no-match | 嵌入维度不匹配,按不相似处理",
            a.len(),
            b.len()
        );
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    // 截断到 [-1, 1] — 两个向量应该已经是单位归一化的，
    // 但浮点误差可能会稍微超出范围。
    dot.clamp(-1.0, 1.0)
}
