// crates/scrollery-ai-core/src/ocr/dict.rs
//! OCR 字符字典 + rec/dict 契约自检。
//!
//! # 映射契约
//! 模型输出 idx 0 = CTC blank;idx `1..=N` → 字典行 `0..N-1`。
//! rec 输出末维 `C` 必须(二选一):
//!
//! - `C == N + 1` → 无空格字符(`use_space = false`);
//! - `C == N + 2` → `use_space = true`,此时 idx `N+1` = `" "`(PaddleOCR `use_space_char`)。
//!
//! 否则报 [`AiError::Ocr`]——这是坑8(词表错配 → 输出乱码)的 OCR 同型防线:
//! 拒绝带病服务而非静默算错。

use std::path::Path;

use crate::error::{AiError, Result};

/// 一份已加载的 OCR 字典(每行一个字符;不含 blank/space,它们由契约推导)。
#[derive(Debug, Clone)]
pub struct OcrDict {
    /// 字典字符行,顺序即索引(行 0 → 模型 idx 1)。
    chars: Vec<String>,
}

impl OcrDict {
    /// UTF-8 逐行读:容 BOM;仅去行尾 `\r\n`;中间空行保留为合法字符行;
    /// 文件末尾单个换行不产生尾随空行(同 Python `readlines` 语义)。
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)
            .map_err(|e| AiError::Ocr(format!("read dict {path:?} failed: {e}")))?;
        let text = String::from_utf8(bytes)
            .map_err(|e| AiError::Ocr(format!("dict {path:?} not UTF-8: {e}")))?;
        Ok(Self::from_str_contents(&text))
    }

    /// 从字符串内容构造(供 [`OcrDict::load`] 与单测复用)。
    pub(crate) fn from_str_contents(text: &str) -> Self {
        // 去 UTF-8 BOM。
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        // 末尾单个 '\n' 视为终止符而非分隔符(避免尾随空行)。
        let body = text.strip_suffix('\n').unwrap_or(text);
        let chars = body
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
            .collect();
        Self { chars }
    }

    /// 字典字符数 N(不含 blank/space)。
    pub fn len(&self) -> usize {
        self.chars.len()
    }

    /// 空字典判定(clippy::len_without_is_empty)。
    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    /// 契约自检:给定模型输出末维 `c`,返回 `use_space`,否则报错配。
    pub fn class_check(&self, c: usize) -> Result<bool> {
        let n = self.chars.len();
        if c == n + 1 {
            Ok(false)
        } else if c == n + 2 {
            Ok(true)
        } else {
            Err(AiError::Ocr(format!(
                "dict/rec class mismatch: model C={c}, dict N={n} (expect N+1={} or N+2={}) | \
                 字典与识别模型错配,拒绝带病服务(坑8 同型)",
                n + 1,
                n + 2
            )))
        }
    }

    /// 按模型 idx 取字符:0 = blank → None;`1..=N` → 字典字符;`use_space && idx==N+1` → `" "`。
    pub fn char(&self, idx: usize, use_space: bool) -> Option<&str> {
        let n = self.chars.len();
        if idx == 0 {
            None
        } else if idx <= n {
            Some(self.chars[idx - 1].as_str())
        } else if use_space && idx == n + 1 {
            Some(" ")
        } else {
            None
        }
    }
}
