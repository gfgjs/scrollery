// src-tauri/src/reader/encoding.rs
//! txt 编码自动识别 + 解码 seam（阅读器完善方案 §5.1）。
//!
//! 现状缺陷:`get_document_text`/`read_ref_text` 用 `std::fs::read_to_string`,非 UTF-8 文件
//! （GBK/GB18030/Big5/Shift_JIS…）直接 `InvalidData` 报错,连带阅读/编辑/版本/AI 校对全废;
//! 前端 `fetch().text()` 兜底又按 UTF-8 强解出乱码。本模块是「一次修三链路」的 seam:
//! 字节 → 检测编码（可被每书 prefs 覆盖）→ 解码为 UTF-8 String + 元数据(编码名/置信度/替换率)。
//!
//! 检测顺序(顺序本身是正确性关键,见各步注释):
//!   1. 手动覆盖(每书 prefs)—— 尊重用户;
//!   2. BOM(UTF-8 / UTF-16LE / UTF-16BE)—— 命中即定,零歧义;
//!   3. 无 BOM 的 UTF-16 零字节启发 —— chardetng **有意不测 UTF-16**,必须自己补;
//!   4. chardetng(喂前 64KB)—— Firefox 同源检测器,覆盖 GBK/Big5/SJIS/单字节族。

use encoding_rs::Encoding;

/// 替换率(U+FFFD 占比)告警阈值:超过即把置信度降级为 `Lossy`,UI 据此亮「编码可能误判」。
/// 0.5% 取自方案 §5.1;正常解码近 0,误判编码通常远超此值。
const REPLACEMENT_WARN_THRESHOLD: f32 = 0.005;

/// chardetng 采样上限:前 64KB。取值理由(方案 §5.1):Firefox 用 1KB 首猜、VS Code 用 64KB 上限、
/// legado 用 8KB——64KB 是折中,因为中文小说开头常有大段 ASCII 广告/版权声明,样本太短会误判。
const SNIFF_SAMPLE_BYTES: usize = 64 * 1024;

/// UTF-16 零字节启发的采样上限(无需扫全文,4KB 足以判断 ASCII 密集的 UTF-16)。
const UTF16_SNIFF_BYTES: usize = 4096;

/// 编码检测的置信度来源。落库 `text_book_index.confidence`,供 UI 审计与「可能误判」提示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// 命中 BOM,零歧义。
    Bom,
    /// 启发式检测(UTF-16 零字节启发 或 chardetng)。
    Detected,
    /// 用户在每书 prefs 中手动指定的编码。
    Manual,
    /// 替换率超阈 —— 无论来源如何,解码产生大量 U+FFFD,大概率编码错误。
    Lossy,
}

impl Confidence {
    /// 落库/传前端的稳定小写标识(text_book_index.confidence 值域)。
    pub fn as_str(self) -> &'static str {
        match self {
            Confidence::Bom => "bom",
            Confidence::Detected => "detected",
            Confidence::Manual => "manual",
            Confidence::Lossy => "lossy",
        }
    }
}

/// 解码结果:UTF-8 文本 + 供审计/UI 的元数据。
#[derive(Debug, Clone)]
pub struct DecodeOutcome {
    /// 解码后的 UTF-8 文本(已剥离 BOM)。
    pub text: String,
    /// 实际生效的编码(单一事实源)。取规范名用 `encoding.name()`(如 "GBK"/"UTF-8"/"Big5");
    /// 分章模块用它扫源文件行首字节偏移(须知道是否 ASCII-兼容 / UTF-16 端序)。
    pub encoding: &'static Encoding,
    /// 置信度来源。
    pub confidence: Confidence,
    /// U+FFFD 替换字符占总字符数的比例(0.0–1.0)。
    pub replacement_ratio: f32,
}

/// 解码字节流为 UTF-8。`override_label` 为每书 prefs 的手动编码覆盖(如 "gb18030");
/// 传 `None` 走自动检测。无效的覆盖标签会被忽略并回落自动检测(不 panic)。
pub fn decode_bytes(bytes: &[u8], override_label: Option<&str>) -> DecodeOutcome {
    // 1) 手动覆盖优先。encoding_rs 用 WHATWG label 查表(大小写/别名容错,如 "gbk"/"GB2312"→GBK)。
    if let Some(label) = override_label {
        if let Some(enc) = Encoding::for_label(label.as_bytes()) {
            return finish_decode(bytes, enc, Confidence::Manual);
        }
        // 覆盖标签无效 → 落自动检测(下方),不因坏配置卡住。
    }

    // 2) BOM:命中即定。for_bom 识别 UTF-8 / UTF-16LE / UTF-16BE 的 BOM。
    if let Some((enc, _bom_len)) = Encoding::for_bom(bytes) {
        return finish_decode(bytes, enc, Confidence::Bom);
    }

    // 3) 无 BOM 的 UTF-16 启发(chardetng 不测 UTF-16,必须自己补)。
    if let Some(enc) = sniff_utf16_no_bom(bytes) {
        return finish_decode(bytes, enc, Confidence::Detected);
    }

    // 4) chardetng:喂前 64KB。
    let enc = sniff_chardetng(bytes);
    finish_decode(bytes, enc, Confidence::Detected)
}

/// 用给定编码解码 + 统计替换率;替换率超阈则把置信度降级为 `Lossy`。
fn finish_decode(bytes: &[u8], enc: &'static Encoding, base: Confidence) -> DecodeOutcome {
    // encoding_rs 的 decode 会自动识别并剥离 BOM(若字节以某 BOM 开头,用 BOM 指示的编码);
    // 对 BOM 分支这与我们已检出的 enc 一致,不会双重处理。actual = 实际生效编码。
    let (text, actual, _had_errors) = enc.decode(bytes);
    let replacement_ratio = replacement_ratio(&text);
    // Lossy 覆盖一切来源:即便是 BOM/Manual,只要产生大量 U+FFFD 就说明有问题,给 UI 最可行动的信号。
    let confidence = if replacement_ratio > REPLACEMENT_WARN_THRESHOLD {
        Confidence::Lossy
    } else {
        base
    };
    DecodeOutcome {
        text: text.into_owned(),
        encoding: actual,
        confidence,
        replacement_ratio,
    }
}

/// U+FFFD(替换字符)占总字符数的比例。空文本记 0。
fn replacement_ratio(text: &str) -> f32 {
    let total = text.chars().count();
    if total == 0 {
        return 0.0;
    }
    let repl = text.chars().filter(|&c| c == '\u{FFFD}').count();
    repl as f32 / total as f32
}

/// chardetng 检测:喂前 64KB,`last=true` 让它对(可能被截断的)样本给出最终结论。
/// `guess(None, true)`:无 TLD 提示;`allow_utf8=true`——独立桌面 app 无浏览器「禁把网页误判 UTF-8」
/// 的顾虑,允许 UTF-8 作为候选(纯 UTF-8 无 BOM 的 txt 常见)。
fn sniff_chardetng(bytes: &[u8]) -> &'static Encoding {
    let mut detector = chardetng::EncodingDetector::new();
    let truncated = bytes.len() > SNIFF_SAMPLE_BYTES;
    let sample = &bytes[..bytes.len().min(SNIFF_SAMPLE_BYTES)];
    // last = !truncated(2026-07-17 实证修复):截断采样时不得声称流已结束 —— 64KB 切点
    // 大概率落在 UTF-8 多字节序列中间,last=true 会让 chardetng 把被截断的尾序列判为
    // UTF-8 非法,进而把 >64KB 纯 CJK 无 BOM 的 UTF-8 整册误判 windows-1252;且 1252
    // 每个字节都可映射 → 替换率恒 0,Lossy 告警也不响,mojibake 完全静默。
    // last=false 下 chardetng 把不完整尾序列当「后续还有数据」缓冲,UTF-8 候选保留。
    detector.feed(sample, !truncated);
    detector.guess(None, true)
}

/// 无 BOM 的 UTF-16 启发:靠「高字节全零」的奇偶分布判定。
/// ASCII 密集的 UTF-16LE 文本高字节在**奇数**下标(近全零);UTF-16BE 高字节在**偶数**下标。
/// 只在**强信号**(一侧零占比 ≥0.30、另一侧 <0.05)时判定 —— 单字节中文(GBK/Big5)几乎无零字节,
/// 不会误命中;弱信号一律回落 chardetng。CJK 密集的无 BOM UTF-16 零字节少,可能漏判,属方案
/// 明示的低概率兜底(§5.1:中文 txt 场景 UTF-16 占比极低,失败走替换率告警)。
fn sniff_utf16_no_bom(bytes: &[u8]) -> Option<&'static Encoding> {
    let sample = &bytes[..bytes.len().min(UTF16_SNIFF_BYTES)];
    if sample.len() < 16 {
        return None;
    }
    let mut zero_even = 0usize; // 偶数下标(0,2,4…)为零字节的数量
    let mut zero_odd = 0usize; // 奇数下标(1,3,5…)为零字节的数量
    let mut pairs = 0usize;
    let mut i = 0;
    while i + 1 < sample.len() {
        if sample[i] == 0 {
            zero_even += 1;
        }
        if sample[i + 1] == 0 {
            zero_odd += 1;
        }
        pairs += 1;
        i += 2;
    }
    if pairs == 0 {
        return None;
    }
    let even_ratio = zero_even as f32 / pairs as f32;
    let odd_ratio = zero_odd as f32 / pairs as f32;
    if odd_ratio >= 0.30 && even_ratio < 0.05 {
        Some(encoding_rs::UTF_16LE)
    } else if even_ratio >= 0.30 && odd_ratio < 0.05 {
        Some(encoding_rs::UTF_16BE)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用 encoding_rs 的编码器把已知中文串编成目标编码字节(免提交二进制 fixture)。
    /// 注意:**不可用于 UTF-16** —— 按 WHATWG,encoding_rs 的 UTF-16 编码器被替换为 UTF-8 编码器,
    /// `UTF_16LE.encode()` 返回的是 UTF-8 字节。UTF-16 fixture 用下面的 to_utf16* 手工构造。
    fn encode(enc: &'static Encoding, s: &str) -> Vec<u8> {
        let (bytes, _, _) = enc.encode(s);
        bytes.into_owned()
    }

    /// 手工构造 UTF-16LE 字节(不含 BOM)。
    fn to_utf16le(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
    }

    /// 手工构造 UTF-16BE 字节(不含 BOM)。
    fn to_utf16be(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()
    }

    const SAMPLE: &str = "第一章 风起\n洛阳城的清晨,薄雾还未散去。他提着灯笼,慢慢走过青石板路。\nThe quick brown fox。";

    #[test]
    fn detects_and_decodes_gbk() {
        let bytes = encode(encoding_rs::GBK, SAMPLE);
        let out = decode_bytes(&bytes, None);
        assert_eq!(out.text, SAMPLE, "GBK 解码须无损还原");
        assert_eq!(out.encoding.name(), "GBK");
        assert!(out.replacement_ratio < REPLACEMENT_WARN_THRESHOLD);
        assert_ne!(out.confidence, Confidence::Lossy);
    }

    #[test]
    fn detects_gb18030_four_byte() {
        // GB18030 4 字节区字符(如 U+00E9 é 在 gb18030 是 4 字节);WHATWG 下 GBK decoder = gb18030 decoder,
        // 报成 GBK 也能正确解 4 字节序列(方案 §5.1 明示无害)。
        let s = "第二章 café 咖啡馆的秘密";
        let bytes = encode(encoding_rs::GB18030, s);
        let out = decode_bytes(&bytes, None);
        assert_eq!(out.text, s, "GB18030 4 字节序列须无损还原");
        assert!(out.replacement_ratio < REPLACEMENT_WARN_THRESHOLD);
    }

    #[test]
    fn detects_and_decodes_big5() {
        let s = "第一回 繁體中文測試,他說:「今天天氣真好。」";
        let bytes = encode(encoding_rs::BIG5, s);
        let out = decode_bytes(&bytes, None);
        assert_eq!(out.text, s, "Big5 解码须无损还原");
        assert!(out.replacement_ratio < REPLACEMENT_WARN_THRESHOLD);
    }

    #[test]
    fn detects_shift_jis() {
        let s = "第一章 日本語のテスト、これはサンプルです。";
        let bytes = encode(encoding_rs::SHIFT_JIS, s);
        let out = decode_bytes(&bytes, None);
        assert_eq!(out.text, s, "Shift_JIS 解码须无损还原");
        assert!(out.replacement_ratio < REPLACEMENT_WARN_THRESHOLD);
    }

    #[test]
    fn utf8_with_bom_stripped() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF]; // UTF-8 BOM
        bytes.extend_from_slice(SAMPLE.as_bytes());
        let out = decode_bytes(&bytes, None);
        assert_eq!(out.confidence, Confidence::Bom);
        assert_eq!(out.text, SAMPLE, "BOM 必须被剥离,不残留 U+FEFF");
        assert!(!out.text.starts_with('\u{FEFF}'));
    }

    #[test]
    fn utf16le_with_bom() {
        let mut with_bom = vec![0xFF, 0xFE]; // UTF-16LE BOM
        with_bom.extend_from_slice(&to_utf16le(SAMPLE));
        let out = decode_bytes(&with_bom, None);
        assert_eq!(out.confidence, Confidence::Bom);
        assert_eq!(out.text, SAMPLE);
    }

    #[test]
    fn utf16be_with_bom() {
        let mut with_bom = vec![0xFE, 0xFF]; // UTF-16BE BOM
        with_bom.extend_from_slice(&to_utf16be(SAMPLE));
        let out = decode_bytes(&with_bom, None);
        assert_eq!(out.confidence, Confidence::Bom);
        assert_eq!(out.text, SAMPLE);
    }

    #[test]
    fn utf16le_no_bom_via_heuristic() {
        // ASCII 密集内容 → 无 BOM 的 UTF-16LE 有强零字节奇偶信号(高字节全零在奇数位),启发式应命中。
        let s = "Chapter One\nThe quick brown fox jumps over the lazy dog.\nHello world, this is a test.";
        let bytes = to_utf16le(s);
        assert!(!bytes.starts_with(&[0xFF, 0xFE]), "构造的字节不应含 BOM");
        let out = decode_bytes(&bytes, None);
        assert_eq!(out.text, s, "无 BOM UTF-16LE 应经零字节启发正确解码");
        assert_eq!(out.encoding.name(), "UTF-16LE");
    }

    #[test]
    fn manual_override_respected() {
        // 一段在 GBK 与 Windows-1252 下都「能解但不同」的字节:强制 override 应生效。
        let bytes = encode(encoding_rs::GBK, "测试");
        let out = decode_bytes(&bytes, Some("gbk"));
        assert_eq!(out.confidence, Confidence::Manual);
        assert_eq!(out.text, "测试");
    }

    #[test]
    fn invalid_override_falls_back_to_detection() {
        let bytes = encode(encoding_rs::GBK, SAMPLE);
        // 无效标签 → 回落自动检测,不 panic、不返回空。
        let out = decode_bytes(&bytes, Some("not-a-real-encoding"));
        assert_eq!(out.text, SAMPLE);
        assert_ne!(out.confidence, Confidence::Manual);
    }

    #[test]
    fn wrong_encoding_flags_lossy() {
        // 把 GBK 字节强制按 Shift_JIS 解 → 大量 U+FFFD → 置信度 Lossy。
        let bytes = encode(
            encoding_rs::GBK,
            "洛阳城的清晨薄雾还未散去他提着灯笼慢慢走过青石板路",
        );
        let out = decode_bytes(&bytes, Some("shift_jis"));
        assert!(
            out.replacement_ratio > REPLACEMENT_WARN_THRESHOLD
                || out.confidence == Confidence::Lossy,
            "错误编码应触发 Lossy 告警(替换率 {})",
            out.replacement_ratio
        );
    }

    #[test]
    fn large_utf8_cjk_beyond_sniff_window_detects_utf8() {
        // >64KB 纯 CJK UTF-8(无 BOM):64KB 采样切点大概率落在 3 字节序列中间。
        // 回归防线:截断采样须 feed(last=false),否则 chardetng 判 UTF-8 非法 →
        // windows-1252 静默 mojibake(替换率 0,Lossy 不响)。
        let text = "第一章 风起云涌,山雨欲来。".repeat(6000);
        assert!(text.len() > SNIFF_SAMPLE_BYTES, "fixture 须超过采样窗");
        let out = decode_bytes(text.as_bytes(), None);
        assert_eq!(out.encoding.name(), "UTF-8");
        assert_eq!(out.text, text, "大体量无 BOM UTF-8 必须无损还原");
    }

    #[test]
    fn plain_ascii_and_utf8() {
        let out = decode_bytes(SAMPLE.as_bytes(), None);
        assert_eq!(out.text, SAMPLE);
        assert!(out.replacement_ratio < REPLACEMENT_WARN_THRESHOLD);
    }

    #[test]
    fn empty_input_is_safe() {
        let out = decode_bytes(&[], None);
        assert_eq!(out.text, "");
        assert_eq!(out.replacement_ratio, 0.0);
    }
}
