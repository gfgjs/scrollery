// src-tauri/src/utils/hash.rs
//! 哈希实用工具：xxHash3 缓存键与 SHA-256 hex 完整性校验。
//!
//! 兼容键为 `xxh3_64("{rel_path}/{file_name}|{file_mtime}")`；当前扫描键包含数据库
//! 目录身份和纳秒 mtime，避免不同扫描根内的同名文件共用缓存，结果存储为 i64。
//! 当用作文件名时：`format!("{:016x}", cache_key as u64)` 以避免出现负号。

use sha2::{Digest as _, Sha256};
use std::path::Path;
use xxhash_rust::xxh3::xxh3_64;

/// 计算旧格式媒体项缓存键（仅供读取/迁移旧调用点）。
///
/// - `rel_path`: 在扫描根目录内的相对路径（如果项目在根级别则为空字符串）
/// - `file_name`: 文件的基本名称
/// - `file_mtime`: 最后修改时间的 Unix 时间戳
///
/// 返回一个 i64（由 u64 重新解释位）。
pub fn compute_cache_key(rel_path: &str, file_name: &str, file_mtime: i64) -> i64 {
    compute_cache_key_impl(None, rel_path, file_name, file_mtime, None)
}

/// 计算包含目录身份和纳秒 mtime 的媒体项缓存键。
///
/// `directory_id` 在同一数据库内唯一且不依赖盘符，隔离不同扫描根中同名、同时间的文件。
/// 纳秒值是缓存路径的一部分，避免同秒同大小改写时，迟到的旧 worker 仍能原子替换
/// 当前 worker 使用的同一最终文件。`0` 表示平台无法提供纳秒精度，仍显式写入版本化
/// 输入以免与旧三字段键混用。
pub fn compute_cache_key_with_mtime_ns(
    directory_id: i64,
    rel_path: &str,
    file_name: &str,
    file_mtime: i64,
    file_mtime_ns: i64,
) -> i64 {
    compute_cache_key_impl(
        Some(directory_id),
        rel_path,
        file_name,
        file_mtime,
        Some(file_mtime_ns),
    )
}

fn compute_cache_key_impl(
    directory_id: Option<i64>,
    rel_path: &str,
    file_name: &str,
    file_mtime: i64,
    file_mtime_ns: Option<i64>,
) -> i64 {
    // 构建稳定的输入序列。当 rel_path 为空时使用前导 "/"，
    // 使得格式始终为 "{rel_path}/{file_name}|{mtime}"。
    // 扫描热路径(百万文件)优先在 512 字节栈缓冲内完成拼接,避免逐文件 format!/Vec 分配;
    // 超长路径回退 Vec；目录身份使用定长字节前缀，不增加逐文件字符串分配。
    const STACK_BUF: usize = 512;
    let mut mtime_buf = [0u8; 20];
    let remaining = {
        use std::io::Write as _;
        let mut cursor = &mut mtime_buf[..];
        write!(cursor, "{file_mtime}").expect("20-byte stack buffer always fits i64 decimal");
        cursor.len()
    };
    let mtime_len = 20 - remaining;
    let mut mtime_ns_buf = [0u8; 20];
    let mtime_ns_len = if let Some(file_mtime_ns) = file_mtime_ns {
        let remaining = {
            use std::io::Write as _;
            let mut cursor = &mut mtime_ns_buf[..];
            write!(cursor, "{file_mtime_ns}")
                .expect("20-byte stack buffer always fits i64 decimal");
            cursor.len()
        };
        Some(20 - remaining)
    } else {
        None
    };
    const NS_MARKER: &[u8] = b"|ns=";
    const DIRECTORY_MARKER: &[u8] = b"directory-v1\0";
    let directory_bytes = directory_id.map(i64::to_le_bytes);
    let rel = rel_path.as_bytes();
    let name = file_name.as_bytes();
    let total_len = if rel.is_empty() {
        1 + name.len() + 1 + mtime_len + mtime_ns_len.map_or(0, |len| NS_MARKER.len() + len)
    } else {
        rel.len()
            + 1
            + name.len()
            + 1
            + mtime_len
            + mtime_ns_len.map_or(0, |len| NS_MARKER.len() + len)
    };
    let total_len = total_len + directory_bytes.map_or(0, |_| DIRECTORY_MARKER.len() + 8);
    let mut stack = [0u8; STACK_BUF];
    if total_len <= STACK_BUF {
        let mut n = 0;
        if let Some(directory_bytes) = directory_bytes {
            stack[..DIRECTORY_MARKER.len()].copy_from_slice(DIRECTORY_MARKER);
            n += DIRECTORY_MARKER.len();
            stack[n..n + directory_bytes.len()].copy_from_slice(&directory_bytes);
            n += directory_bytes.len();
        }
        if !rel.is_empty() {
            stack[n..n + rel.len()].copy_from_slice(rel);
            n += rel.len();
        }
        stack[n] = b'/';
        n += 1;
        stack[n..n + name.len()].copy_from_slice(name);
        n += name.len();
        stack[n] = b'|';
        n += 1;
        stack[n..n + mtime_len].copy_from_slice(&mtime_buf[..mtime_len]);
        n += mtime_len;
        if let Some(ns_len) = mtime_ns_len {
            stack[n..n + NS_MARKER.len()].copy_from_slice(NS_MARKER);
            n += NS_MARKER.len();
            stack[n..n + ns_len].copy_from_slice(&mtime_ns_buf[..ns_len]);
            n += ns_len;
        }
        xxh3_64(&stack[..n]) as i64
    } else {
        let mut bytes = Vec::with_capacity(total_len);
        if let Some(directory_bytes) = directory_bytes {
            bytes.extend_from_slice(DIRECTORY_MARKER);
            bytes.extend_from_slice(&directory_bytes);
        }
        if !rel.is_empty() {
            bytes.extend_from_slice(rel);
        }
        bytes.push(b'/');
        bytes.extend_from_slice(name);
        bytes.push(b'|');
        bytes.extend_from_slice(&mtime_buf[..mtime_len]);
        if let Some(ns_len) = mtime_ns_len {
            bytes.extend_from_slice(NS_MARKER);
            bytes.extend_from_slice(&mtime_ns_buf[..ns_len]);
        }
        xxh3_64(&bytes) as i64
    }
}

/// 将 cache_key i64 转换为文件名中使用的十六进制字符串。
/// 使用无符号位模式以避免前导 `-` 符号。
pub fn cache_key_to_hex(cache_key: i64) -> String {
    format!("{:016x}", cache_key as u64)
}

// ── SHA-256 → 小写 hex(R2-6:原 download/exotic 五处重复实现收拢) ─────────────

/// 字节序列 → 小写 hex 字符串。
pub fn to_hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 字节 sha256(64 位小写 hex,与 exotic/package.rs is_sha256_hex 校验器契约一致)。
pub fn sha256_hex(bytes: &[u8]) -> String {
    to_hex_lower(&Sha256::digest(bytes))
}

/// 文件 sha256(小写 hex)。64 KiB 分块流式读,不全量载入内存(支持 GB 级文件)。
pub fn sha256_hex_of_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read as _;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1 << 16];
    loop {
        match file.read(&mut buf)? {
            0 => break,
            n => hasher.update(&buf[..n]),
        }
    }
    Ok(to_hex_lower(&hasher.finalize()))
}

/// 内容指纹全文阈值：超出后不读内容，直接交由扫描保守判定源变更。
pub const CONTENT_HASH_FULL_LIMIT: u64 = 64 * 1024 * 1024;

/// 媒体可疑变更的完整内容指纹，格式为 `sha256:<hex>`。
///
/// 大于 64 MiB 返回 `None` 且不打开文件：抽样无法证明内容未变，扫描本就须保守失效，
/// 继续计算抽样摘要不会改变判定。旧抽样基线由定案时清除。
pub fn content_fingerprint(path: &Path, file_size: i64) -> std::io::Result<Option<String>> {
    if file_size.max(0) as u64 > CONTENT_HASH_FULL_LIMIT {
        return Ok(None);
    }
    Ok(Some(format!("sha256:{}", sha256_hex_of_file(path)?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_cache_namespace_covers_stack_and_long_paths() {
        let expected = |directory_id: i64, rel: &str| {
            let mut bytes = b"directory-v1\0".to_vec();
            bytes.extend_from_slice(&directory_id.to_le_bytes());
            bytes.extend_from_slice(
                format!("{rel}/same.bmp|1700000000|ns=1700000000123456700").as_bytes(),
            );
            xxh3_64(&bytes) as i64
        };
        let first = compute_cache_key_with_mtime_ns(
            1,
            "photos",
            "same.bmp",
            1_700_000_000,
            1_700_000_000_123_456_700,
        );
        let second = compute_cache_key_with_mtime_ns(
            2,
            "photos",
            "same.bmp",
            1_700_000_000,
            1_700_000_000_123_456_700,
        );
        assert_eq!(first, expected(1, "photos"));
        assert_eq!(second, expected(2, "photos"));
        assert_ne!(first, second, "相同相对路径和时间必须按目录身份隔离");
        let long_path = "层级/".repeat(80);
        assert_eq!(
            compute_cache_key_with_mtime_ns(
                2,
                &long_path,
                "same.bmp",
                1_700_000_000,
                1_700_000_000_123_456_700
            ),
            expected(2, &long_path),
            "超出栈缓冲的路径保持相同字节契约"
        );
    }
}
