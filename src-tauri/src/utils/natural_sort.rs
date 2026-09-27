// src-tauri/src/utils/natural_sort.rs
//! 溢出安全的「自然序」字符串比较（数字感知:`"50" < "100"`、`"file2" < "file10"`）。
//!
//! ## 背景（为何自研替代 lexicmp）
//! 原用 `lexicmp 0.2.0::natural_cmp` 注册为 SQLite 的 `NATURAL_CMP` collation（见
//! [`crate::db::register_custom_collations`]）。它把数字段逐位累加进 `u64`（`n = n*10 + digit`,
//! **无 checked/saturating**）;当两个被比较的名字在同位都出现 **≥20 位连续数字**（真实来源:
//! 小红书导出 `Camera_XHS_1702005…`、哈希/时间戳文件名）时,`n*10` 超过 `u64::MAX`（20 位十进制）
//! → debug 构建（overflow-checks=on）panic「attempt to multiply with overflow」。该 panic 被
//! rusqlite 的 collation `catch_unwind` 兜住（返回 -1、进程不崩）,但默认 panic hook 仍逐比较刷
//! stderr、且栈展开代价在 O(N log N) 次排序比较里被成千放大 → 刷屏 + 排序塌方。上游 issue
//! surrealdb/lexicmp#1 自 2023-09 开放至今未修（PR#2 停滞）,0.2.0 已是最新版 → 升级无解,故以
//! 本函数替换,并把 lexicmp 降为 `[dev-dependencies]` 仅作对拍基准（生产二进制不再含它）。
//!
//! ## 排序契约（与 `lexicmp::natural_cmp` 逐输入等价,仅去掉溢出）
//! 逐字符扫描两串:
//! - 两侧当前字符**都是 ASCII 数字** → 进入数字段比较（见 [`cmp_digit_run`]）:数字段**不解析
//!   成整数**,而是 lockstep 同步消费——**较长的数字段更大**（无前导零时即数值更大）,等长时
//!   逐位字典序（等长数字串的字典序 == 数值序）。这与 lexicmp 的「长度短路 + 等长 `n1.cmp(n2)`」
//!   语义**逐输入一致**(含前导零的长度优先怪癖 `"7" < "07" < "007"` 一并保留,以免改动既有排序
//!   视图),但对任意位数免疫溢出。
//! - 否则字符不等 → 返回码点序（`char::cmp`,ASCII < 多字节 UTF-8,与文件树 BINARY /
//!   `TREE_SORT_KEY` 字节序约定一致）;相等 → 前进。
//! - 一侧先耗尽 → 短者为小（前缀更短更靠前:`"ab" < "abc"`）。
//!
//! release 构建下 overflow-checks 默认关,原 lexicmp 不 panic 但 `n*10` 静默 wrap → ≥20 位数字名
//! 的自然序本就是**错的**（只是不炸）。本函数一并修正该潜在正确性 bug。
//!
//! 单测以 lexicmp 为**对拍基准**（`#[cfg(test)]`,仅测试编译）钉死「非溢出输入逐项等价」,另有
//! 显式特征化用例锁定 ≥20 位数字名、前导零、纯数字/混字母等边界。

use std::cmp::Ordering;
use std::iter::Peekable;
use std::str::Chars;

/// 自然序比较:数字段按数值大小、其余按码点序。数字感知（`"2" < "10"`）,对任意长数字段免疫溢出。
///
/// 契约细节与自研缘由见模块级文档。签名 `Fn(&str, &str) -> Ordering` 直接作为 `NATURAL_CMP`
/// collation 的回调（见 [`crate::db::register_custom_collations`]）。
pub fn natural_cmp(s1: &str, s2: &str) -> Ordering {
    let mut i1 = s1.chars().peekable();
    let mut i2 = s2.chars().peekable();
    loop {
        match (i1.next(), i2.next()) {
            (Some(c1), Some(c2)) => {
                if c1.is_ascii_digit() && c2.is_ascii_digit() {
                    match cmp_digit_run(c1, c2, &mut i1, &mut i2) {
                        // 两数字段相等 → 继续比较其后字符（如 `"a01b"` vs `"a1c"`:01==1 后再比 b/c）。
                        Ordering::Equal => continue,
                        non_eq => return non_eq,
                    }
                } else if c1 != c2 {
                    return c1.cmp(&c2);
                }
                // 相等的非数字字符 → 前进到下一位。
            }
            // 一侧耗尽:短者为小。
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (None, None) => return Ordering::Equal,
        }
    }
}

/// 比较两个数字段（各自首位数字 `first1`/`first2` 已被外层消费,迭代器停在首位之后的字符）。
///
/// **溢出安全**:不把数字段解析成定长整数,而是 lockstep 同步消费两侧后续数字——
/// - 一侧数字先耗尽（另一侧仍是数字）→ 数字段更长者更大（无前导零时即数值更大）;
/// - 两侧同时耗尽 → 返回等长期间记录的「首个差异位」码点序（等长数字串字典序 == 数值序）。
///
/// 与 lexicmp `cmp_ascii_digits!` 宏逐输入等价:该宏在一侧数字耗尽处按长度短路,两侧等长时以
/// `n1.cmp(n2)` 比值（等长即字典序）。此处以「首个差异位」replay 等长字典序,免去 `n*10` 溢出。
/// 返回 `Equal` 表示两数字段完全相等,外层据此继续比较数字段之后的字符。
fn cmp_digit_run(
    first1: char,
    first2: char,
    i1: &mut Peekable<Chars<'_>>,
    i2: &mut Peekable<Chars<'_>>,
) -> Ordering {
    // 等长情形的裁决:首个差异位的码点序（首位相等则顺延到后续首个差异位,恰为数值高位优先）。
    let mut lexical = first1.cmp(&first2);
    loop {
        let d1 = i1.peek().copied().filter(|c| c.is_ascii_digit());
        let d2 = i2.peek().copied().filter(|c| c.is_ascii_digit());
        match (d1, d2) {
            (Some(a), Some(b)) => {
                if lexical == Ordering::Equal {
                    lexical = a.cmp(&b);
                }
                i1.next();
                i2.next();
            }
            // 数字段长度不等:更长者更大（长度优先,覆盖任何等长字典裁决）。
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            // 等长:返回等长期间的首个差异位序（全等 → Equal,外层继续比较后续字符）。
            (None, None) => return lexical,
        }
    }
}
