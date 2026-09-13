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

/// 把一章的解码文本切分为段落数组。`reflow=false` 走保守一级;`true` 叠加二级智能重排。
/// 返回的每段已去首尾空白(缩进交给 CSS);空段被过滤。
pub fn segment(chapter_text: &str, reflow: bool) -> Vec<String> {
    // 一级:非空行即段(行首缩进空白由 trim 去除,渲染层再统一施加 text-indent)。
    let lines: Vec<&str> = chapter_text
        .split('\n')
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    if !reflow {
        return lines.into_iter().map(|s| s.to_string()).collect();
    }
    reflow_lines(&lines)
}

/// 二级智能重排:行尾非句末标点 → 与下一行黏合(CJK 无空格连接);句末标点处断段。
/// 幂等:输出各段(除末段)均以句末标点收尾,再次重排产出相同结果(见测试 `reflow_is_idempotent`)。
fn reflow_lines(lines: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut buf = String::new();
    for &line in lines {
        // CJK 场景无空格连接;若缓冲与新行衔接处两侧都是 ASCII 字母/数字,补一个空格避免粘连英文单词。
        if !buf.is_empty() && needs_space_join(&buf, line) {
            buf.push(' ');
        }
        buf.push_str(line);
        if ends_sentence(&buf) {
            out.push(std::mem::take(&mut buf));
        }
    }
    if !buf.is_empty() {
        out.push(buf);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level1_one_paragraph_per_line() {
        let text = "第一段文字。\n第二段文字。\n第三段文字。";
        let paras = segment(text, false);
        assert_eq!(paras, vec!["第一段文字。", "第二段文字。", "第三段文字。"]);
    }

    #[test]
    fn level1_blank_lines_separate_and_indent_stripped() {
        // 空行分隔 + 行首全角/半角缩进应被剥离(缩进交给 CSS)。
        let text = "\u{3000}\u{3000}有缩进的一段。\n\n\n  两个半角空格缩进的一段。\n";
        let paras = segment(text, false);
        assert_eq!(paras, vec!["有缩进的一段。", "两个半角空格缩进的一段。"]);
    }

    #[test]
    fn level1_does_not_merge() {
        // 一级绝不合并:即使硬换行断句,也各自成段(交给二级去处理)。
        let text = "这是被硬换行\n切断的一句话。\n下一段。";
        let paras = segment(text, false);
        assert_eq!(paras.len(), 3);
    }

    #[test]
    fn reflow_merges_hard_wrapped_lines() {
        // 前两行是被硬换行切断的一句(第一行不以句末标点结尾)→ 应黏合为一段。
        let text = "这是被硬换行\n切断的一句话。\n这是完整的下一段。";
        let paras = segment(text, true);
        assert_eq!(
            paras,
            vec!["这是被硬换行切断的一句话。", "这是完整的下一段。"]
        );
    }

    #[test]
    fn reflow_keeps_sentence_boundaries() {
        // 每行都以句末标点结尾 → 重排不应合并。
        let text = "第一句。\n第二句!\n第三句?";
        let paras = segment(text, true);
        assert_eq!(paras, vec!["第一句。", "第二句!", "第三句?"]);
    }

    #[test]
    fn reflow_respects_closing_quote_as_ender() {
        // 句末标点在右引号内:「…好。」结尾的 」 属句末集,应断段。
        let text = "他说:\u{300C}今天天气真好。\u{300D}\n新的一段开始了。";
        let paras = segment(text, true);
        assert_eq!(paras.len(), 2);
        assert!(paras[0].ends_with('\u{300D}'));
    }

    #[test]
    fn reflow_is_idempotent() {
        // 幂等:重排两次 = 一次(把重排结果用 \n 拼回再重排,须得同一结果)。
        let text = "被拆开\n的一句\n话结束了。\n另一句\n也被拆\n开了!";
        let once = segment(text, true);
        let rejoined = once.join("\n");
        let twice = segment(&rejoined, true);
        assert_eq!(once, twice, "重排应幂等");
    }

    #[test]
    fn reflow_english_gets_space_join() {
        // 英文硬换行黏合时,单词间应补空格(不粘成一个词)。
        let text = "The quick brown\nfox jumps.";
        let paras = segment(text, true);
        assert_eq!(paras, vec!["The quick brown fox jumps."]);
    }

    #[test]
    fn empty_and_whitespace_only() {
        assert!(segment("", false).is_empty());
        assert!(segment("\n\n  \n\u{3000}\n", false).is_empty());
        assert!(segment("", true).is_empty());
    }

    /// B2 回归钉:标题行无句末标点,reflow 曾把它与下一行黏成「第一章 风起洛阳城的清晨…」。
    /// 行级预剥离后,重排首段应从正文开始且标题不再出现。
    #[test]
    fn strip_title_before_reflow_keeps_body_clean() {
        let text = "第一章 风起\n洛阳城的清晨,雾还没散。\n人已经上路。";
        let body = strip_title_line(text, "第一章 风起");
        let paras = segment(body, true);
        assert_eq!(paras, vec!["洛阳城的清晨,雾还没散。", "人已经上路。"]);
    }

    #[test]
    fn strip_title_line_exact_match_only() {
        // 精确匹配才剥;不匹配(如 clip 截断的超长标题)原样返回,宁可重复不误删正文。
        let text = "正文首行不是标题。\n第二行。";
        assert_eq!(strip_title_line(text, "某个标题"), text);
        // 前导空行被跨越,标题行连同其换行一起剥离。
        let text2 = "\n  第一章 风起\n正文。";
        assert_eq!(strip_title_line(text2, "第一章 风起"), "正文。");
    }

    #[test]
    fn strip_title_line_no_reflow_parity() {
        // 非重排路径:剥离后一级分段结果与旧「分段后比对首段」行为一致。
        let text = "第二章 云涌\n第一段。\n第二段。";
        let paras = segment(strip_title_line(text, "第二章 云涌"), false);
        assert_eq!(paras, vec!["第一段。", "第二段。"]);
    }
}
