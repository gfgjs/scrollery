//! 导出文件命名档与文件名合规化(方案 A §2.3)。纯函数,无 IO/DB,便于单测穷举边界。

/// 命名档(方案 §2.3)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NamingScheme {
    /// 原文件名。
    Original,
    /// `001-原名.jpg`,位宽 `max(3, total 位数)`,保留手排/视图顺序。
    Sequence,
    /// `YYYYMMDD_HHmmss-原名.jpg`,取 `sort_datetime`(UTC,与全库月桶/日期分组同一时区惯例)。
    Date,
}

/// Windows 保留设备名(不区分大小写,匹配「首个 `.` 之前的整段」——`CON.txt` 非法,
/// `condition.txt` 合法)。
const RESERVED_STEMS: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Windows 官方文档与 `CON`/`COM1` 并列列出的遗留 OEM 代码页上标变体保留名
/// (`COM¹/COM²/COM³`、`LPT¹/LPT²/LPT³`)。这些是非 ASCII 码点,`eq_ignore_ascii_case`
/// 对其无效(无大小写可折),故单独精确匹配(审查 P3)。
const RESERVED_STEMS_SUPERSCRIPT: &[&str] = &[
    "COM\u{00B9}",
    "COM\u{00B2}",
    "COM\u{00B3}",
    "LPT\u{00B9}",
    "LPT\u{00B2}",
    "LPT\u{00B3}",
];

/// 合规化单个文件名分量(方案 §2.3:保留名 / 非法字符 / 结尾空格与点 / 空 stem)。
/// 不处理大小写不敏感冲突与路径过长——那是跨项目录级别的状态,留给调用方在复制循环中处理。
pub fn sanitize_component(name: &str) -> String {
    // 非法字符与控制字符 → 下划线(Windows 非法集 `<>:"/\|?*` + 0x00-0x1F;macOS/Linux
    // 更宽松,但导出目标未知,按最严平台统一处理以保证跨平台可用)。
    let mut out: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();

    // 结尾空格/点在 Windows 上会被静默剥离甚至导致创建失败——显式剥离并留痕(不产生歧义)。
    while out.ends_with('.') || out.ends_with(' ') {
        out.pop();
    }

    if out.is_empty() {
        out = "_".to_string();
    }

    // 保留设备名判定按「首个 `.` 之前」的整段,不区分大小写;上标变体额外精确匹配。
    let stem = out.split('.').next().unwrap_or(&out);
    if RESERVED_STEMS.iter().any(|r| r.eq_ignore_ascii_case(stem))
        || RESERVED_STEMS_SUPERSCRIPT.contains(&stem)
    {
        out = format!("_{out}");
    }

    out
}

/// 序号位宽:`max(3, total 的十进制位数)`(方案 §2.3)。
fn sequence_width(total: usize) -> usize {
    let digits = total.max(1).to_string().len();
    digits.max(3)
}

/// 按命名档构建目标文件名(1-based `index`)。`original_name` 须已是单一文件名组件
/// (调用方在取 DB 值时校验,§2.3);本函数只做合规化,不做路径穿越校验。
pub fn build_file_name(
    scheme: NamingScheme,
    index: usize,
    total: usize,
    original_name: &str,
    sort_datetime_utc_secs: i64,
) -> String {
    let safe_original = sanitize_component(original_name);
    match scheme {
        NamingScheme::Original => safe_original,
        NamingScheme::Sequence => {
            let width = sequence_width(total);
            format!("{index:0width$}-{safe_original}")
        }
        NamingScheme::Date => {
            let dt = chrono::DateTime::from_timestamp(sort_datetime_utc_secs, 0)
                .unwrap_or_else(|| chrono::DateTime::from_timestamp(0, 0).unwrap());
            format!("{}-{safe_original}", dt.format("%Y%m%d_%H%M%S"))
        }
    }
}

/// 大小写不敏感文件系统(Windows/macOS 默认)去重:同名(忽略大小写)时按冲突策略处理。
/// 返回 `None` 表示应跳过(policy=Skip 命中冲突);`Some(name)` 是最终落盘用文件名。
/// `used`(小写化)由调用方在成功落盘后 `insert`。
pub fn resolve_conflict(
    candidate: &str,
    used: &std::collections::HashSet<String>,
    allow_rename: bool,
) -> Option<String> {
    let key = candidate.to_lowercase();
    if !used.contains(&key) {
        return Some(candidate.to_string());
    }
    if !allow_rename {
        return None; // policy=Skip
    }
    // 在 stem/扩展名之间插入 `-2`、`-3`...,直到不冲突。
    let (stem, ext) = match candidate.rfind('.') {
        Some(i) if i > 0 => (&candidate[..i], &candidate[i..]),
        _ => (candidate, ""),
    };
    for n in 2..10_000 {
        let attempt = format!("{stem}-{n}{ext}");
        if !used.contains(&attempt.to_lowercase()) {
            return Some(attempt);
        }
    }
    None // 理论不可达(近万次同名冲突);兜底当作 Skip
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_illegal_chars() {
        assert_eq!(sanitize_component("a:b*c?.jpg"), "a_b_c_.jpg");
    }

    #[test]
    fn sanitize_trailing_dot_space() {
        assert_eq!(sanitize_component("name. . "), "name");
    }

    #[test]
    fn sanitize_empty_stem_fallback() {
        assert_eq!(sanitize_component("..."), "_");
        assert_eq!(sanitize_component(""), "_");
    }

    #[test]
    fn sanitize_reserved_name_case_insensitive() {
        assert_eq!(sanitize_component("con.txt"), "_con.txt");
        assert_eq!(sanitize_component("CON"), "_CON");
        assert_eq!(sanitize_component("Con.JPG"), "_Con.JPG");
        // 非精确匹配不受影响
        assert_eq!(sanitize_component("condition.txt"), "condition.txt");
        assert_eq!(sanitize_component("LPT10.txt"), "LPT10.txt");
    }

    /// 审查 P3:Windows 官方文档列出的遗留 OEM 代码页上标变体(`COM¹/²/³`、`LPT¹/²/³`)同样
    /// 是保留名,精确匹配(非 ASCII,无大小写可折)。
    #[test]
    fn sanitize_reserved_superscript_com_lpt_variants() {
        assert_eq!(sanitize_component("COM\u{00B9}.txt"), "_COM\u{00B9}.txt");
        assert_eq!(sanitize_component("LPT\u{00B2}"), "_LPT\u{00B2}");
        assert_eq!(sanitize_component("COM\u{00B3}"), "_COM\u{00B3}");
    }

    #[test]
    fn sequence_width_matches_total_digits_min3() {
        assert_eq!(sequence_width(5), 3);
        assert_eq!(sequence_width(999), 3);
        assert_eq!(sequence_width(1000), 4);
        assert_eq!(sequence_width(123456), 6);
    }

    #[test]
    fn build_file_name_sequence() {
        let n = build_file_name(NamingScheme::Sequence, 1, 5, "IMG_0001.jpg", 0);
        assert_eq!(n, "001-IMG_0001.jpg");
        // total=12345 → 5 位数,width=max(3,5)=5。
        let n = build_file_name(NamingScheme::Sequence, 42, 12345, "a.jpg", 0);
        assert_eq!(n, "00042-a.jpg");
    }

    #[test]
    fn build_file_name_date_is_utc() {
        // 2025-01-02 03:04:05 UTC
        let ts = 1735787045;
        let n = build_file_name(NamingScheme::Date, 1, 1, "a.jpg", ts);
        assert_eq!(n, "20250102_030405-a.jpg");
    }

    #[test]
    fn resolve_conflict_rename_increments() {
        let mut used = std::collections::HashSet::new();
        used.insert("a.jpg".to_string());
        used.insert("a-2.jpg".to_string());
        let r = resolve_conflict("a.jpg", &used, true).unwrap();
        assert_eq!(r, "a-3.jpg");
    }

    #[test]
    fn resolve_conflict_skip_returns_none() {
        let mut used = std::collections::HashSet::new();
        used.insert("a.jpg".to_string());
        assert_eq!(resolve_conflict("a.jpg", &used, false), None);
    }

    #[test]
    fn resolve_conflict_case_insensitive() {
        let mut used = std::collections::HashSet::new();
        used.insert("a.jpg".to_string());
        let r = resolve_conflict("A.JPG", &used, true).unwrap();
        assert_eq!(r, "A-2.JPG");
    }
}
