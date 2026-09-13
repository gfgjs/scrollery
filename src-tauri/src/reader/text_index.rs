// src-tauri/src/reader/text_index.rs
//! txt 智能分章（阅读器完善方案 §5.2 / R1-3）。
//!
//! 产物 = 章节索引(标题 + 源文件字节区间 + 章内字符数)。前端只见「章序号 + 标题 + 字符数」,
//! 字节偏移留在 Rust(get_text_chapter 按区间 seek 解码,IPC 载荷恒为单章级)。
//!
//! 机制仿 legado(GPL,**只借鉴机制,规则自写**,见 txt_toc_rules.json):
//!   - 内置一组带 priority/deny_suffix 的正则规则(数据文件,为将来「用户可编辑规则」留门);
//!   - 选规则:前 512KB 逐规则统计命中(相邻 <1000 字符只计一次,防正文连续引用误判),
//!     命中最多者当选,平手按 priority;命中不足阈值 → 伪章兜底;
//!   - 分章:当选规则跑全文,命中位置(天然在行首)即章界。
//!
//! **字节偏移对齐**(正确性核心):章界永在行首(紧跟换行)。ASCII 兼容编码的 `0x0A` 绝不出现在
//! 多字节序列内部(GBK/Big5/SJIS 尾字节范围不含 0x0A),故可直接扫源字节得行首偏移,且
//! `decoded.split('\n')` 第 i 行恰好对齐第 i 个源行首(每个源换行解码为且仅为一个 '\n')。
//! UTF-16 按端序 2 字节对齐扫换行码元。→ 一次全量解码 + 一次字节扫描即得源字节偏移。

use std::sync::OnceLock;

use encoding_rs::Encoding;
use regex::Regex;
use serde::{Deserialize, Serialize};

use super::encoding as enc_seam;

/// 分章标题最大字符数(超出截断;章题过长多为误命中的正文行)。
const MAX_TITLE_CHARS: usize = 60;
/// 选规则采样字节数(方案 §5.2)。
const SELECT_SAMPLE_BYTES: usize = 512 * 1024;
/// 选规则相邻命中去重的字符距离(防正文连续引用「第X章」刷高某规则分数)。
const ADJ_DEDUP_CHARS: usize = 1000;
/// 采信一条规则的最低命中数(样本内)。低于此视为误命中,走伪章兜底。
const MIN_HITS_TRUST: usize = 2;
/// 伪章(无规则命中兜底)的目标字符长度。
const PSEUDO_CHARS: usize = 10_000;
/// 规则命中章的字符数上限(2026-07-17 内存爆炸修复)。稀疏命中(如日志文本偶合章题正则)可产
/// 多 MB 巨章 —— 前端一章 = 一个 foliate section,超大 section 在 paginated multicol 下有实测
/// GB 级内存放大(测量见 docs/worklogs/2026-07-17-md阅读器巨型文档内存爆炸修复/findings.md;
/// md 侧同因由 syntheticBook MD_SECTION_BUDGET_CHARS 治)。超限章按 PSEUDO_CHARS 粒度在行边界
/// 续切,标题「原题 · N」保导航语义。
const MAX_RULE_CHAPTER_CHARS: usize = 30_000;

/// 一章的元数据。序列化为 DB `text_book_index.chapters` 的 JSON 数组(键 t/s/e/n 省空间)。
/// `byte_start`/`byte_end` 是**源文件字节偏移**(位于源编码的字符边界,可安全切片再解码)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChapterMeta {
    #[serde(rename = "t")]
    pub title: String,
    #[serde(rename = "s")]
    pub byte_start: usize,
    #[serde(rename = "e")]
    pub byte_end: usize,
    #[serde(rename = "n")]
    pub char_len: usize,
}

/// 分章索引的完整构建结果。encoding/confidence 落 DB 同名列,chapters 落 JSON 列。
#[derive(Debug, Clone)]
pub struct TextBookIndex {
    /// 最终编码规范名(如 "GBK"/"UTF-8")。
    pub encoding: String,
    /// 置信度来源:bom/detected/manual/lossy。
    pub confidence: String,
    /// U+FFFD 替换率(不落库,仅供日志/可选透传;confidence='lossy' 已编码「超阈」事实)。
    pub replacement_ratio: f32,
    pub chapters: Vec<ChapterMeta>,
}

// ── 规则集(编译期嵌入 + 首用编译缓存)──────────────────────────────────────────
// include_str! 而非 tauri resource:满足「数据文件 + 将来可编辑」而免运行时资源路径脆弱性;
// 将来 pro「用户可编辑规则」时,改为「内置默认 + 从 DB/磁盘读覆盖」即可,seam 不变。

const RULES_JSON: &str = include_str!("txt_toc_rules.json");

#[derive(Deserialize)]
struct RawRule {
    name: String,
    pattern: String,
    #[serde(default)]
    deny_suffix: String,
    #[serde(default = "default_priority")]
    priority: i64,
    #[serde(default = "default_enabled")]
    enabled: bool,
}
fn default_priority() -> i64 {
    50
}
fn default_enabled() -> bool {
    true
}

struct CompiledRule {
    #[allow(dead_code)] // 保留供日志/将来规则编辑器展示
    name: String,
    re: Regex,
    /// 匹配结束位置紧邻字符若在此集合内则否决匹配(负向断言等价,规避 Rust regex 无 lookaround)。
    deny_suffix: String,
    priority: i64,
}

impl CompiledRule {
    /// 该规则是否判定给定行为章节标题行。line 已按 '\n' 切分,可能带尾随 '\r'。
    fn matches(&self, line: &str) -> bool {
        let line = line.trim_end_matches('\r');
        match self.re.find(line) {
            Some(m) => {
                if self.deny_suffix.is_empty() {
                    return true;
                }
                // 检查匹配结束处紧邻的字符是否被排除(如 节 后是「课」→ 「第三节课」不是章)。
                match line[m.end()..].chars().next() {
                    Some(c) => !self.deny_suffix.contains(c),
                    None => true, // 行尾无后继字符 → 不排除
                }
            }
            None => false,
        }
    }
}

static RULES: OnceLock<Vec<CompiledRule>> = OnceLock::new();

/// 加载并编译规则集(首用一次,之后复用)。启用项才编译;正则编译失败的规则跳过(不致命)。
fn load_rules() -> &'static [CompiledRule] {
    RULES.get_or_init(|| {
        let raw: Vec<RawRule> = serde_json::from_str(RULES_JSON).unwrap_or_else(|e| {
            tracing::error!("txt 分章规则集解析失败,降级为空(全走伪章兜底)| {e}");
            Vec::new()
        });
        raw.into_iter()
            .filter(|r| r.enabled)
            .filter_map(|r| match Regex::new(&r.pattern) {
                Ok(re) => Some(CompiledRule {
                    name: r.name,
                    re,
                    deny_suffix: r.deny_suffix,
                    priority: r.priority,
                }),
                Err(e) => {
                    tracing::warn!("分章规则 [{}] 正则编译失败,跳过 | {e}", r.name);
                    None
                }
            })
            .collect()
    })
}

// ── 构建入口 ────────────────────────────────────────────────────────────────

/// 从源字节构建分章索引。`override_label` = 每书 prefs 的手动编码覆盖(R1 恒 None,R3 接入)。
pub fn build_index(source_bytes: &[u8], override_label: Option<&str>) -> TextBookIndex {
    let outcome = enc_seam::decode_bytes(source_bytes, override_label);
    let enc = outcome.encoding;
    let decoded = &outcome.text;

    let src_starts = source_line_starts(source_bytes, enc);
    let dec_lines: Vec<&str> = decoded.split('\n').collect();
    // 两向量应等长(见模块注释论证);为防御异常编码下的失配,后续一切索引都 clamp 到 min。
    debug_assert_eq!(
        src_starts.len(),
        dec_lines.len(),
        "源行首数应与解码行数对齐"
    );

    let rules = load_rules();
    let selected = select_rule(rules, &src_starts, &dec_lines, source_bytes.len());

    let mut chapters = match selected {
        Some(rule) => split_by_rule(rule, &src_starts, &dec_lines, source_bytes.len()),
        None => Vec::new(),
    };
    if chapters.is_empty() {
        chapters = pseudo_chapters(&src_starts, &dec_lines, source_bytes.len());
    }
    // 最终兜底:恒至少一章(空文件 → 一空章),避免前端拿到零章。
    if chapters.is_empty() {
        chapters.push(ChapterMeta {
            title: "正文".to_string(),
            byte_start: 0,
            byte_end: source_bytes.len(),
            char_len: decoded.chars().count(),
        });
    }

    TextBookIndex {
        encoding: enc.name().to_string(),
        confidence: outcome.confidence.as_str().to_string(),
        replacement_ratio: outcome.replacement_ratio,
        chapters,
    }
}

/// 计算源文件每行行首的字节偏移。ASCII 兼容编码按 LF(0x0A)扫;UTF-16 按端序 2 字节对齐扫换行码元。
fn source_line_starts(bytes: &[u8], enc: &'static Encoding) -> Vec<usize> {
    let mut starts = vec![0usize];
    if enc == encoding_rs::UTF_16LE {
        let mut i = 0;
        while i + 1 < bytes.len() {
            if bytes[i] == 0x0A && bytes[i + 1] == 0x00 {
                starts.push(i + 2);
            }
            i += 2;
        }
    } else if enc == encoding_rs::UTF_16BE {
        let mut i = 0;
        while i + 1 < bytes.len() {
            if bytes[i] == 0x00 && bytes[i + 1] == 0x0A {
                starts.push(i + 2);
            }
            i += 2;
        }
    } else {
        // ASCII 兼容:LF 是无歧义行分隔(GBK/Big5/SJIS 尾字节不含 0x0A)。
        for (i, &b) in bytes.iter().enumerate() {
            if b == 0x0A {
                starts.push(i + 1);
            }
        }
    }
    starts
}

/// 选规则:前 512KB 逐启用规则统计去重后命中数,命中最多者当选,平手按 priority;不足阈值→None。
fn select_rule<'a>(
    rules: &'a [CompiledRule],
    src_starts: &[usize],
    dec_lines: &[&str],
    src_len: usize,
) -> Option<&'a CompiledRule> {
    let n = src_starts.len().min(dec_lines.len());
    if n == 0 {
        return None;
    }
    // 样本行数:src_start < 512KB 的行(只看字节偏移,不数字符,廉价)。
    let sample_limit = SELECT_SAMPLE_BYTES.min(src_len).max(1);
    let mut sample_end = 0usize;
    while sample_end < n && src_starts[sample_end] < sample_limit {
        sample_end += 1;
    }
    let sample_end = sample_end.clamp(1, n);
    // 样本内各行解码字符起始偏移(+1 近似换行,用于相邻去重距离判定)。
    let mut char_off = Vec::with_capacity(sample_end);
    let mut acc = 0usize;
    for &line in dec_lines.iter().take(sample_end) {
        char_off.push(acc);
        acc += line.chars().count() + 1;
    }

    let mut best: Option<(&CompiledRule, usize)> = None;
    for rule in rules {
        let mut hits = 0usize;
        let mut last: Option<usize> = None;
        for i in 0..sample_end {
            if rule.matches(dec_lines[i]) {
                let off = char_off[i];
                if last.is_none_or(|l| off.saturating_sub(l) >= ADJ_DEDUP_CHARS) {
                    hits += 1;
                    last = Some(off);
                }
            }
        }
        if hits == 0 {
            continue;
        }
        best = match best {
            None => Some((rule, hits)),
            Some((_, bh)) if hits > bh => Some((rule, hits)),
            Some((br, bh)) if hits == bh && rule.priority > br.priority => Some((rule, hits)),
            keep => keep,
        };
    }
    best.filter(|&(_, hits)| hits >= MIN_HITS_TRUST)
        .map(|(rule, _)| rule)
}

/// 用当选规则跑全文切章。
fn split_by_rule(
    rule: &CompiledRule,
    src_starts: &[usize],
    dec_lines: &[&str],
    src_len: usize,
) -> Vec<ChapterMeta> {
    let n = src_starts.len().min(dec_lines.len());
    let marks: Vec<usize> = (0..n).filter(|&i| rule.matches(dec_lines[i])).collect();
    if marks.is_empty() {
        return Vec::new();
    }
    let mut chapters = Vec::with_capacity(marks.len() + 1);
    // 前言块:首个章界前若有实质内容,独立成章(否则读者会丢失开头正文)。
    if marks[0] > 0 && has_nonspace(dec_lines, 0, marks[0]) {
        push_chapter_capped(
            &mut chapters,
            first_nonempty_snippet(dec_lines, 0, marks[0]).unwrap_or_else(|| "前言".into()),
            src_starts,
            dec_lines,
            0,
            marks[0],
            src_len,
        );
    }
    for (k, &m) in marks.iter().enumerate() {
        let end_line = marks.get(k + 1).copied().unwrap_or(n);
        push_chapter_capped(
            &mut chapters,
            clip_title(dec_lines[m]),
            src_starts,
            dec_lines,
            m,
            end_line,
            src_len,
        );
    }
    chapters
}

/// 把行区间 `[start_line, end_line)` 作为一章推入;超过 MAX_RULE_CHAPTER_CHARS 则按 PSEUDO_CHARS
/// 粒度在行边界续切为「原题 · N」子章(见常量注释:巨章 = 前端巨型 section = multicol 内存放大)。
fn push_chapter_capped(
    chapters: &mut Vec<ChapterMeta>,
    title: String,
    src_starts: &[usize],
    dec_lines: &[&str],
    start_line: usize,
    end_line: usize,
    src_len: usize,
) {
    if count_chars(dec_lines, start_line, end_line) <= MAX_RULE_CHAPTER_CHARS {
        chapters.push(ChapterMeta {
            title,
            byte_start: src_starts[start_line],
            byte_end: line_byte_end(src_starts, end_line, src_len),
            char_len: count_chars(dec_lines, start_line, end_line),
        });
        return;
    }
    // 与 pseudo_chapters 同款行边界累积切分,但保留原章题前缀作导航锚。
    let mut seg_start = start_line;
    let mut seg_chars = 0usize;
    let mut part = 1usize;
    let hard_end = end_line.min(dec_lines.len());
    for (off, line) in dec_lines[start_line..hard_end].iter().enumerate() {
        let i = start_line + off;
        seg_chars += line.chars().count() + 1;
        if seg_chars >= PSEUDO_CHARS && i + 1 < end_line {
            chapters.push(ChapterMeta {
                title: format!("{title} · {part}"),
                byte_start: src_starts[seg_start],
                byte_end: line_byte_end(src_starts, i + 1, src_len),
                char_len: seg_chars,
            });
            part += 1;
            seg_start = i + 1;
            seg_chars = 0;
        }
    }
    if seg_start < end_line {
        chapters.push(ChapterMeta {
            title: format!("{title} · {part}"),
            byte_start: src_starts[seg_start],
            byte_end: line_byte_end(src_starts, end_line, src_len),
            char_len: seg_chars,
        });
    }
}

/// 伪章兜底:无规则命中时按 ~10K 字符在行边界切段。
fn pseudo_chapters(src_starts: &[usize], dec_lines: &[&str], src_len: usize) -> Vec<ChapterMeta> {
    let n = src_starts.len().min(dec_lines.len());
    if n == 0 {
        return Vec::new();
    }
    let mut chapters = Vec::new();
    let mut seg_start = 0usize;
    let mut seg_chars = 0usize;
    let mut idx = 1usize;
    for i in 0..n {
        seg_chars += dec_lines[i].chars().count() + 1;
        if seg_chars >= PSEUDO_CHARS && i + 1 < n {
            let end_line = i + 1;
            chapters.push(make_pseudo(
                src_starts, dec_lines, seg_start, end_line, idx, src_len,
            ));
            idx += 1;
            seg_start = end_line;
            seg_chars = 0;
        }
    }
    if seg_start < n {
        chapters.push(make_pseudo(
            src_starts, dec_lines, seg_start, n, idx, src_len,
        ));
    }
    chapters
}

fn make_pseudo(
    src_starts: &[usize],
    dec_lines: &[&str],
    start_line: usize,
    end_line: usize,
    idx: usize,
    src_len: usize,
) -> ChapterMeta {
    ChapterMeta {
        title: first_nonempty_snippet(dec_lines, start_line, end_line)
            .unwrap_or_else(|| format!("片段 {idx}")),
        byte_start: src_starts[start_line.min(src_starts.len().saturating_sub(1))],
        byte_end: line_byte_end(src_starts, end_line, src_len),
        char_len: count_chars(dec_lines, start_line, end_line),
    }
}

// ── 小工具 ──────────────────────────────────────────────────────────────────

/// 行区间 [start, end) 结束处的源字节偏移:end 越界则取文件末尾。
fn line_byte_end(src_starts: &[usize], end_line: usize, src_len: usize) -> usize {
    if end_line < src_starts.len() {
        src_starts[end_line]
    } else {
        src_len
    }
}

/// 行区间字符数(含近似换行 +1/行)。仅作展示提示,不参与切片,近似可接受。
fn count_chars(dec_lines: &[&str], start: usize, end: usize) -> usize {
    let end = end.min(dec_lines.len());
    (start..end).map(|i| dec_lines[i].chars().count() + 1).sum()
}

/// 行区间内是否存在非空白字符。
fn has_nonspace(dec_lines: &[&str], start: usize, end: usize) -> bool {
    let end = end.min(dec_lines.len());
    (start..end).any(|i| dec_lines[i].chars().any(|c| !c.is_whitespace()))
}

/// 区间首个非空行的截断快照(伪章/前言块标题用)。
fn first_nonempty_snippet(dec_lines: &[&str], start: usize, end: usize) -> Option<String> {
    let end = end.min(dec_lines.len());
    (start..end)
        .map(|i| dec_lines[i].trim())
        .find(|s| !s.is_empty())
        .map(clip_title)
}

/// 章标题清洗:去首尾空白(含尾随 \r)+ 按字符截断到 MAX_TITLE_CHARS。
fn clip_title<S: AsRef<str>>(line: S) -> String {
    let t = line.as_ref().trim();
    if t.is_empty() {
        return "无题".to_string();
    }
    let mut out: String = t.chars().take(MAX_TITLE_CHARS).collect();
    if t.chars().count() > MAX_TITLE_CHARS {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用给定编码把文本编成源字节,构建索引,再对每章「切源字节区间→解码」对拍标题,验证偏移正确。
    fn roundtrip_check(index: &TextBookIndex, source: &[u8]) {
        let enc = Encoding::for_label(index.encoding.as_bytes()).expect("已知编码");
        for ch in &index.chapters {
            assert!(ch.byte_start <= ch.byte_end, "章区间应有序");
            assert!(ch.byte_end <= source.len(), "章区间不越界");
            let (slice_text, _, _) = enc.decode(&source[ch.byte_start..ch.byte_end]);
            // 章文本(去空白)应以章标题打头(标题取自该章首行);标题可能被截断,故用 starts_with 前缀判定。
            let head: String = slice_text
                .trim_start()
                .chars()
                .take(ch.title.chars().count())
                .collect();
            let title_no_ellipsis = ch.title.trim_end_matches('…');
            assert!(
                head.starts_with(title_no_ellipsis)
                    || ch.title == "前言"
                    || ch.title.starts_with("片段")
                    // 巨章续切的「原题 · N」子章:N≥2 的切片起点在章体中部,标题是合成导航锚
                    // 而非首行快照,不做前缀对拍(区间有序/越界断言仍全量生效)。
                    || ch.title.contains(" · "),
                "章「{}」偏移对拍失败:切片开头为「{}」",
                ch.title,
                slice_text.trim_start().chars().take(20).collect::<String>()
            );
        }
    }

    /// 单行填充正文(~1440 字符,无内部换行),用于把测试章节撑到超过 1000 字符去重窗口,
    /// 使「选规则」阶段的相邻去重不把紧邻的测试章节折叠掉 —— 代表真实小说的章间距。
    fn filler_para() -> String {
        "填充正文内容,足够长以超过相邻命中去重窗口。".repeat(80)
    }

    /// 造一本「真实间距」的书:每章 = 标题行 + 一行长填充正文,章间以 \n 相连。
    fn make_book(titles: &[&str]) -> String {
        let f = filler_para();
        titles
            .iter()
            .map(|t| format!("{t}\n{f}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn splits_chinese_chapters_utf8() {
        let text = make_book(&["第一章 风起", "第二章 云涌", "第三章 雨落"]);
        let index = build_index(text.as_bytes(), None);
        let titles: Vec<&str> = index.chapters.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, vec!["第一章 风起", "第二章 云涌", "第三章 雨落"]);
        roundtrip_check(&index, text.as_bytes());
    }

    #[test]
    fn leading_preamble_becomes_chapter() {
        let f = filler_para();
        // 前言(无标记,两行实质内容)+ 两个真实章节(满足选规则 ≥2 命中)。
        let text = format!(
            "这是一段没有章节标记的开头引言。\n继续引言内容。\n第一章 开始\n{f}\n第二章 继续\n{f}"
        );
        let index = build_index(text.as_bytes(), None);
        assert_eq!(index.chapters.len(), 3, "前言 + 第一章 + 第二章");
        assert_eq!(index.chapters[1].title, "第一章 开始");
        assert_eq!(index.chapters[0].byte_start, 0, "前言块从文件头起");
        roundtrip_check(&index, text.as_bytes());
    }

    #[test]
    fn deny_suffix_rejects_false_positive() {
        let f = filler_para();
        // 行首「第三节课…」应被 deny_suffix(课)否决,不成章;三个真实「第X节 …」成章。
        let text = format!(
            "第一节 序幕\n{f}\n第三节课的安排如下。\n{f}\n第二节 发展\n{f}\n第三节 高潮\n{f}"
        );
        let index = build_index(text.as_bytes(), None);
        let titles: Vec<&str> = index.chapters.iter().map(|c| c.title.as_str()).collect();
        assert!(
            !titles.iter().any(|t| t.starts_with("第三节课")),
            "负向断言失效:第三节课被误判为章 | titles={titles:?}"
        );
        assert_eq!(titles, vec!["第一节 序幕", "第二节 发展", "第三节 高潮"]);
    }

    #[test]
    fn gbk_offsets_roundtrip() {
        let text = make_book(&["第一章 洛阳", "第二章 长安", "第三章 江南"]);
        let (bytes, _, _) = encoding_rs::GBK.encode(&text);
        let index = build_index(&bytes, None);
        assert_eq!(index.encoding, "GBK");
        assert_eq!(index.chapters.len(), 3);
        assert_eq!(index.chapters[0].title, "第一章 洛阳");
        // 偏移对拍:切 GBK 源字节区间 + 解码,须还原各章文本。
        roundtrip_check(&index, &bytes);
    }

    #[test]
    fn pseudo_fallback_when_no_rule() {
        // 无任何章节标记 + 多行 + 超过 10K 字符 → 伪章兜底切多段(切点在行边界)。
        let para = "这是一段没有任何章节标记的连续正文。\n".repeat(800);
        let index = build_index(para.as_bytes(), None);
        assert!(
            index.chapters.len() >= 2,
            "长无标记文本应切多个伪章,实得 {}",
            index.chapters.len()
        );
        // 伪章偏移连续无缝、覆盖全文。
        assert_eq!(index.chapters.first().unwrap().byte_start, 0);
        assert_eq!(index.chapters.last().unwrap().byte_end, para.len());
        for w in index.chapters.windows(2) {
            assert_eq!(w[0].byte_end, w[1].byte_start, "伪章区间应首尾相接");
        }
        roundtrip_check(&index, para.as_bytes());
    }

    #[test]
    fn short_text_single_chapter() {
        let text = "很短的一段文字,没有章节。";
        let index = build_index(text.as_bytes(), None);
        assert_eq!(index.chapters.len(), 1);
        assert_eq!(index.chapters[0].byte_start, 0);
        assert_eq!(index.chapters[0].byte_end, text.len());
    }

    #[test]
    fn empty_file_yields_one_chapter() {
        let index = build_index(&[], None);
        assert_eq!(index.chapters.len(), 1);
        assert_eq!(index.chapters[0].byte_start, 0);
        assert_eq!(index.chapters[0].byte_end, 0);
    }

    #[test]
    fn oversized_rule_chapter_gets_capped() {
        let f = filler_para();
        // 巨章:第二章正文 30 段填充(~43K 字符)> MAX_RULE_CHAPTER_CHARS → 按 PSEUDO_CHARS 续切「· N」。
        let giant_body = format!("{f}\n").repeat(30);
        let text = format!("第一章 甲\n{f}\n第二章 乙\n{giant_body}第三章 丙\n{f}");
        let index = build_index(text.as_bytes(), None);
        let titles: Vec<&str> = index.chapters.iter().map(|c| c.title.as_str()).collect();
        // 常规章不受影响。
        assert!(
            titles.contains(&"第一章 甲"),
            "enc={} conf={} titles[0..3]={:?}",
            index.encoding,
            index.confidence,
            &titles[..titles.len().min(3)]
        );
        assert!(titles.contains(&"第三章 丙"), "titles={titles:?}");
        // 巨章切为「第二章 乙 · N」序列,原题单章不复存在。
        let parts = titles
            .iter()
            .filter(|t| t.starts_with("第二章 乙 · "))
            .count();
        assert!(parts >= 3, "巨章应切多个子章,实得 {titles:?}");
        assert!(!titles.contains(&"第二章 乙"), "巨章不应再以原题整章存在");
        // 子章字符数有界(PSEUDO_CHARS 粒度,行边界允许溢出一行 ≈ filler 长度)。
        for ch in index
            .chapters
            .iter()
            .filter(|c| c.title.starts_with("第二章 乙 · "))
        {
            assert!(
                ch.char_len <= PSEUDO_CHARS + 2000,
                "子章过大:{} = {} 字符",
                ch.title,
                ch.char_len
            );
        }
        // 全书区间连续无缝(续切不得制造空洞/重叠)。
        for w in index.chapters.windows(2) {
            assert_eq!(w[0].byte_end, w[1].byte_start, "章区间应首尾相接");
        }
        roundtrip_check(&index, text.as_bytes());
    }

    /// Characterization(审查 F-04):分章只在**行边界**续切——无内部换行的单行巨串
    /// (minified JSON/base64/单行日志)整行独占一章,PSEUDO_CHARS/MAX_RULE_CHAPTER_CHARS
    /// 上限对它失效。这是行边界切分的已知局限:行内切分需 decoded-char→源字节映射,在
    /// 非 UTF-8 编码下是新的偏移正确性风险面,故后端不切;内存放大由前端护栏兜底
    /// (BookReader 检出超限 section 强制 scrolled,阈值 FORCE_SCROLLED_SECTION_CHARS)。
    /// 若未来实现行内安全切分,本测试应当反转。
    #[test]
    fn single_giant_line_stays_one_chapter() {
        // 单行 ~40 万字符,远超 MAX_RULE_CHAPTER_CHARS(30K),但无换行 → 不可续切。
        let giant_line = "abcdefghij".repeat(40_000);
        let index = build_index(giant_line.as_bytes(), None);
        assert_eq!(index.chapters.len(), 1, "单行巨串应保持单章(行边界局限)");
        let ch = &index.chapters[0];
        assert_eq!(ch.byte_start, 0);
        assert_eq!(ch.byte_end, giant_line.len());
        assert!(
            ch.char_len > MAX_RULE_CHAPTER_CHARS,
            "char_len={} 应如实上报超限,供前端护栏判定",
            ch.char_len
        );
    }

    #[test]
    fn crlf_offsets_align() {
        // Windows 换行 \r\n:偏移对齐不能被 \r 打乱;标题不残留 \r。
        let f = filler_para();
        let text = format!("第一章 甲\r\n{f}\r\n第二章 乙\r\n{f}");
        let index = build_index(text.as_bytes(), None);
        assert_eq!(index.chapters.len(), 2);
        assert_eq!(index.chapters[0].title, "第一章 甲");
        roundtrip_check(&index, text.as_bytes());
    }

    #[test]
    fn chapters_serialize_compact_keys() {
        let ch = ChapterMeta {
            title: "第一章".into(),
            byte_start: 0,
            byte_end: 10,
            char_len: 3,
        };
        let json = serde_json::to_string(&ch).unwrap();
        assert!(json.contains("\"t\":"), "应用紧凑键 t");
        assert!(json.contains("\"s\":"));
        assert!(json.contains("\"e\":"));
        assert!(json.contains("\"n\":"));
        // 回环
        let back: ChapterMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ch);
    }

    #[test]
    fn ruleset_loads_and_compiles() {
        // 规则集数据文件须解析成功且至少编译出若干启用规则(默认关的纯数字行不计)。
        let rules = load_rules();
        assert!(
            rules.len() >= 6,
            "规则集应编译出多条启用规则,实得 {}",
            rules.len()
        );
    }
}
