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

    /// 由 SQLite 原始行打包常驻快照:跳过维度 ≠ `dim` 的行(含其它模型/变体的向量)。
    /// `data` 行主序,行序与 `ids` 一致;元素按小端 f32 读入转 f16。
    pub fn pack(model_name: &str, dim: usize, raw: Vec<(i64, Vec<u8>)>) -> Self {
        let mut ids: Vec<i64> = Vec::with_capacity(raw.len());
        let mut data: Vec<f16> = Vec::with_capacity(raw.len() * dim);
        for (id, blob) in raw {
            if blob.len() != dim * 4 {
                continue;
            }
            ids.push(id);
            for chunk in blob.as_chunks::<4>().0 {
                data.push(f16::from_f32(f32::from_le_bytes([
                    chunk[0], chunk[1], chunk[2], chunk[3],
                ])));
            }
        }
        Self {
            model_name: model_name.to_string(),
            ids,
            data,
            dim,
        }
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

    // 按相似度降序排序,取 Top-K。
    scored.sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(top_k);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn blob(values: &[f32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    /// 打包:维度不符的行(含其它模型/变体的向量)必须被跳过,不能混进本快照。
    #[test]
    fn pack_filters_rows_of_other_dims() {
        let cache = EmbeddingCache::pack(
            "m1",
            2,
            vec![
                (1, blob(&[1.0, 0.0])),
                (2, blob(&[0.0, 1.0, 0.0])),
                (3, blob(&[0.5, 0.5])),
            ],
        );
        assert_eq!(cache.ids, vec![1, 3]);
        assert_eq!(cache.dim, 2);
        assert_eq!(cache.len(), 2);
        assert!(!cache.is_empty());
    }

    /// 身份判定按 (模型, 维度) 整体比较。
    #[test]
    fn matches_requires_model_and_dim() {
        let cache = EmbeddingCache::pack("m1", 2, vec![(1, blob(&[1.0, 0.0]))]);
        assert!(cache.matches("m1", 2));
        assert!(!cache.matches("m1", 4), "同模型错维不得复用");
        assert!(!cache.matches("m2", 2), "同维错模型不得复用");
    }

    /// 打分:降序取 Top-K,空库/空 K 返回空。
    #[test]
    fn score_top_k_orders_descending_and_truncates() {
        let cache = EmbeddingCache::pack(
            "m1",
            2,
            vec![
                (1, blob(&[1.0, 0.0])),
                (2, blob(&[0.0, 1.0])),
                (3, blob(&[0.707_106_77, 0.707_106_77])),
            ],
        );
        let top = score_top_k(&cache, &[1.0, 0.0], 2).unwrap();
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].0, 1);
        assert_eq!(top[1].0, 3);
        assert!((top[0].1 - 1.0).abs() < 1e-3);
        assert!((top[1].1 - 0.707).abs() < 1e-2);

        let empty = EmbeddingCache::pack("m1", 2, Vec::new());
        assert!(empty.is_empty());
        assert!(score_top_k(&empty, &[1.0, 0.0], 5).unwrap().is_empty());
        assert!(score_top_k(&cache, &[1.0, 0.0], 0).unwrap().is_empty());
    }

    /// 查询向量与快照维度不符:报错而不是按快照维度索引(越界 panic)。
    #[test]
    fn score_top_k_rejects_dim_mismatch_without_panicking() {
        let cache = EmbeddingCache::pack("m1", 4, vec![(1, blob(&[1.0, 0.0, 0.0, 0.0]))]);
        assert!(score_top_k(&cache, &[1.0, 0.0], 5).is_err(), "短向量须报错");
        assert!(
            score_top_k(&cache, &[1.0, 0.0, 0.0, 0.0, 0.0], 5).is_err(),
            "长向量须报错"
        );
        assert_eq!(
            score_top_k(&cache, &[1.0, 0.0, 0.0, 0.0], 5).unwrap().len(),
            1,
            "同维对照:该拒绝不是「什么都拒」"
        );
    }
}
