// crates/scrollery-ai-core/src/embedding.rs
//! 嵌入向量的字节序与归一化工具；无需 `inference` feature。

/// 就地 L2 归一化向量（返回修改后的输入）。
pub fn l2_normalize(mut v: Vec<f32>) -> Vec<f32> {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-8 {
        v.iter_mut().for_each(|x| *x /= norm);
    }
    v
}

/// 将 `Vec<f32>` 嵌入向量转换为原始字节（小端 IEEE 754）。
pub fn embedding_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for &f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

/// 将原始字节转换回 `Vec<f32>` 嵌入向量。
pub fn bytes_to_embedding(b: &[u8]) -> Vec<f32> {
    // as_chunks::<4>() 的 .1 是尾部不足 4 字节的余数,与原 chunks_exact 同样丢弃。
    b.as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect()
}
