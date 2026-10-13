// src-tauri/src/reader/paragraph.rs
//! txt 智能分段（阅读器完善方案 §5.2 / R1-4）。
//!
//! 两级设计,分离「保守默认」与「激进重排」:
//!   - **一级(默认开,保守)**:每个非空行 = 一段;空行为分隔;行首缩进空白剥离(缩进由 CSS
//!     `text-indent` 施加,不改文本)。适配「一行一段」与「空行分段」两种最常见的 txt 排布。
//!   - **二级「智能重排」(每书 opt-in,默认关)**:处理「硬换行断句」的脏 txt —— 行尾非句末标点
//!     则与下行黏合成一段。会改变文本 → 进度定位须走 text-context 重锚(§5.7),故独立开关。
//!
//! 缩进/段间距是**渲染层 CSS**(text-indent / margin),不在本模块加空格 —— 与 legado 插全角空格的
//! 实现不同,好处是可随设置即时变化、且定位偏移不受影响。本模块只负责「切成哪些段」。

/// 句末标点集。判「一行是否为完整句子结尾」用(二级重排的黏合边界)。
/// 用 `\u{}` 转义硬编码,规避源文件/工具链对全角标点的处理歧义。
const SENTENCE_ENDERS: &[char] = &[
    '\u{3002}', // 。 表意句号
    '\u{FF1F}', // ？ 全角问号
    '\u{FF01}', // ！ 全角叹号
    '?', '!', '.',        // ASCII 句末(英文/混排)
    '\u{2026}', // … 省略号
    '\u{FF5E}', // ～ 全角波浪
    '~', '\u{300D}', // 」 直角引号(右)
    '\u{300F}', // 』 双直角引号(右)
    '\u{201D}', // ” 右双引号
    '\u{2019}', // ’ 右单引号
    '\u{FF09}', // ） 全角右括号
    ')',
];

/// 分段文本及一一对应的源行号；行号相对于传入文本、从零起按 LF 计数。
pub struct SegmentedText {
    /// 保留既有去空白、过滤空行和可选重排后的段落内容。
    pub paragraphs: Vec<String>,
    /// 每段第一条源行；重排黏合多行时保留第一行，避免显示变换影响编辑定位。
    pub source_lines: Vec<usize>,
}

/// 把一章解码文本切为段落及源行号。`reflow=false` 每个非空行一段；`true` 在句末断段。
/// 去空白与重排在同一流程记录位置，不从已变换的段落反查重复原文。
pub fn segment(chapter_text: &str, reflow: bool) -> SegmentedText {
    let mut out = SegmentedText {
        paragraphs: Vec::new(),
        source_lines: Vec::new(),
    };
    let mut buf = String::new();
    let mut start_line = 0;
    for (line_number, raw_line) in chapter_text.split('\n').enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if buf.is_empty() {
            start_line = line_number;
        }
        // 英文硬换行两侧是字母/数字时仍补空格，保持既有重排结果。
        if !buf.is_empty() && needs_space_join(&buf, line) {
            buf.push(' ');
        }
        buf.push_str(line);
        if !reflow || ends_sentence(&buf) {
            out.paragraphs.push(std::mem::take(&mut buf));
            out.source_lines.push(start_line);
        }
    }
    if !buf.is_empty() {
        out.paragraphs.push(buf);
        out.source_lines.push(start_line);
    }
    out
}

/// 缓冲末字符与新行首字符是否都是 ASCII 字母/数字(英文硬换行:补空格防单词粘连)。
fn needs_space_join(buf: &str, next: &str) -> bool {
    let last = buf.chars().next_back();
    let first = next.chars().next();
    matches!((last, first), (Some(a), Some(b)) if a.is_ascii_alphanumeric() && b.is_ascii_alphanumeric())
}

/// 末个非空白字符是否为句末标点。
fn ends_sentence(s: &str) -> bool {
    match s.chars().rev().find(|c| !c.is_whitespace()) {
        Some(c) => SENTENCE_ENDERS.contains(&c),
        None => false,
    }
}

/// 从章节原文按**行**剥离与标题相同的首个非空行(2026-07-10 审查 B2)。
///
/// 必须发生在 [`segment`] 之前:智能重排(reflow)会把无句末标点的标题行与下一行黏合成
/// 一段,分段后再拿首段与标题比对必然失配——重排模式下标题既重复渲染又污染正文首段。
/// 只做**精确匹配**:超长标题被 clip_title 截断('…')后不剥离——伪章(pseudo chapter)的
/// title 同样取自首行正文,模糊/前缀匹配有误删真实正文行的风险,宁可保留重复标题。
pub fn strip_title_line<'a>(chapter_text: &'a str, title: &str) -> &'a str {
    let mut consumed = 0;
    for line in chapter_text.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            consumed += line.len();
            continue;
        }
        if trimmed == title {
            return &chapter_text[consumed + line.len()..];
        }
        return chapter_text;
    }
    chapter_text
}
