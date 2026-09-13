// src-tauri/src/reader/zh_convert.rs
//! 简繁转换（阅读器完善方案 §5.10）。纯 Rust 的 ferrous-opencc(零 C++ 依赖,14 内置 config
//! 词典编译进库)。经 doc_commands 的 `convert_chinese` 命令暴露;前端在**应用替换规则之后**
//! 按章批量调用(保证 §5.13「替换规则 → 简繁 → 渲染」顺序)。
//!
//! 转换只作用于渲染层文本,**不改 canonical 底层文本与章内偏移**(§6.2 定位器锚定 canonical),
//! 故与进度/书签定位互不干扰。

use ferrous_opencc::config::BuiltinConfig;
use ferrous_opencc::OpenCC;

use crate::error::{AppError, Result};

/// 把前端传入的 config 名映射为 ferrous-opencc 内置 config。未知名 → None(命令层报错)。
/// 值域取自 OpenCC 惯用名(小写):首发档位 t2s/s2t/s2tw/s2twp,其余内置项一并支持以备扩展。
fn builtin_config(name: &str) -> Option<BuiltinConfig> {
    Some(match name {
        "s2t" => BuiltinConfig::S2t,     // 简 → 繁
        "t2s" => BuiltinConfig::T2s,     // 繁 → 简
        "s2tw" => BuiltinConfig::S2tw,   // 简 → 繁(台湾正体)
        "tw2s" => BuiltinConfig::Tw2s,   // 台湾正体 → 简
        "s2hk" => BuiltinConfig::S2hk,   // 简 → 繁(香港)
        "hk2s" => BuiltinConfig::Hk2s,   // 香港 → 简
        "s2twp" => BuiltinConfig::S2twp, // 简 → 繁(台湾正体 + 词汇转换:软件→軟體)
        "tw2sp" => BuiltinConfig::Tw2sp, // 台湾 → 简(含词汇)
        "t2tw" => BuiltinConfig::T2tw,   // 繁 → 台湾正体
        "tw2t" => BuiltinConfig::Tw2t,   // 台湾正体 → 繁(OpenCC 标准)
        "t2hk" => BuiltinConfig::T2hk,   // 繁 → 香港
        "hk2t" => BuiltinConfig::Hk2t,   // 香港 → 繁(OpenCC 标准)
        _ => return None,
    })
}

/// 按 config 批量转换简繁。**构建一次 converter 转整批**(每次调用一构建,批内摊薄词典加载成本;
/// 前端按章调用 → 每章一次构建,毫秒级)。config 未知或引擎初始化失败返回 AppError::Internal。
///
/// 性能备注:若将来 profiling 显示词典构建是热点(如书内搜索的全书简繁归一),再引入按 config
/// 缓存 converter(需确认 OpenCC 的 Send+Sync);当前按批构建足够,不预先引入全局状态。
pub fn convert_batch(texts: &[String], config: &str) -> Result<Vec<String>> {
    let cfg = builtin_config(config).ok_or_else(|| {
        AppError::Internal(format!("unknown opencc config: {config} | 未知简繁配置"))
    })?;
    let cc = OpenCC::from_config(cfg)
        .map_err(|e| AppError::Internal(format!("opencc init failed | 简繁引擎初始化失败: {e}")))?;
    Ok(texts.iter().map(|t| cc.convert(t)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simplified_to_traditional() {
        let out = convert_batch(&["开放中文转换".to_string()], "s2t").unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], "開放中文轉換");
    }

    #[test]
    fn traditional_to_simplified() {
        let out = convert_batch(&["開放中文轉換".to_string()], "t2s").unwrap();
        assert_eq!(out[0], "开放中文转换");
    }

    #[test]
    fn taiwan_phrase_conversion_differs_from_plain() {
        // s2twp 含词汇转换:「软件」→「軟體」;而 s2t 仅字形转换 →「軟件」。
        let plain = convert_batch(&["软件".to_string()], "s2t").unwrap();
        let taiwan = convert_batch(&["软件".to_string()], "s2twp").unwrap();
        assert_eq!(plain[0], "軟件", "s2t 仅字形");
        assert_eq!(taiwan[0], "軟體", "s2twp 含台湾词汇");
    }

    #[test]
    fn batch_preserves_order_and_length() {
        let input = vec![
            "第一章".to_string(),
            "洛阳".to_string(),
            "".to_string(), // 空串保留(不 panic、原样返回)
            "长安".to_string(),
        ];
        let out = convert_batch(&input, "s2t").unwrap();
        assert_eq!(out.len(), 4);
        assert_eq!(out[0], "第一章");
        assert_eq!(out[1], "洛陽");
        assert_eq!(out[2], "");
        assert_eq!(out[3], "長安");
    }

    #[test]
    fn unknown_config_errors() {
        let r = convert_batch(&["测试".to_string()], "not-a-config");
        assert!(r.is_err(), "未知 config 应报错而非静默");
    }

    #[test]
    fn empty_batch_is_ok() {
        let out = convert_batch(&[], "s2t").unwrap();
        assert!(out.is_empty());
    }
}
