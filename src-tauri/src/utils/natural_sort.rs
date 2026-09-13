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

#[cfg(test)]
mod tests {
    use super::natural_cmp;
    use std::cmp::Ordering;

    // ── 基础数字感知 ─────────────────────────────────────────────────────────────
    #[test]
    fn basic_numeric_ordering() {
        assert_eq!(natural_cmp("2", "10"), Ordering::Less);
        assert_eq!(natural_cmp("file2", "file10"), Ordering::Less);
        assert_eq!(natural_cmp("img100", "img99"), Ordering::Greater);
        assert_eq!(natural_cmp("50", "100"), Ordering::Less);
        assert_eq!(natural_cmp("abc", "abc"), Ordering::Equal);
    }

    #[test]
    fn self_test_parity_matches_connection_selftest() {
        // 与 connection.rs 的 collation 运行时自测同款断言:自然序 1 < 2 < 10。
        let mut v = vec!["10", "2", "1"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["1", "2", "10"]);
    }

    #[test]
    fn equal_length_digit_run_is_lexicographic() {
        // 等长数字段:逐位字典序 == 数值序。
        assert_eq!(natural_cmp("abc049", "abc050"), Ordering::Less);
        assert_eq!(natural_cmp("v1_23", "v1_24"), Ordering::Less);
    }

    // ── 核心回归:≥20 位数字段不再 panic,且给出正确自然序 ─────────────────────────
    #[test]
    fn long_digit_runs_do_not_panic_and_order_correctly() {
        // 生产库真实文件名（小红书导出,21 位对齐数字段;替换前 lexicmp 在此 panic）。
        let a = "Camera_XHS_170200523737001027401khluwcg5ux5010wvelh0cj73z7.jpg";
        let b = "Camera_XHS_170200523981701027401khluwcg5ux50111rx4z1ksn7xr.jpg";
        // 21 位数字段等长 → 首个差异位在第 10 位（7 < 9）→ a < b,且不 panic。
        assert_eq!(natural_cmp(a, b), Ordering::Less);
        assert_eq!(natural_cmp(b, a), Ordering::Greater);
        assert_eq!(natural_cmp(a, a), Ordering::Equal); // 自反,长段亦然

        // 27 位数字段（全库最长）相邻取值,不 panic。
        let c = "1488946725_201808222102505507635434873.jpg";
        let d = "1488946725_201808222102505507635434874.jpg";
        assert_eq!(natural_cmp(c, d), Ordering::Less);

        // 纯 30 位数字串,远超 u64,仍全序可比。
        let e = "123456789012345678901234567890";
        let f = "123456789012345678901234567899";
        assert_eq!(natural_cmp(e, f), Ordering::Less);
        assert_eq!(natural_cmp(e, e), Ordering::Equal);
    }

    #[test]
    fn longer_digit_run_is_greater_even_beyond_u64() {
        // 长度优先:更长数字段更大,即便两者都远超 u64。
        let short = "img99999999999999999999"; // 20 位
        let long = "img999999999999999999999"; // 21 位
        assert_eq!(natural_cmp(short, long), Ordering::Less);
    }

    // ── 边界:前导零（保留 lexicmp 长度优先语义,避免改动既有视图序）────────────────
    #[test]
    fn leading_zeros_preserve_length_dominant_semantics() {
        // 特征化:数字段更长者更大,即使数值相等——与替换前 lexicmp 逐输入一致（非 bug,是刻意保序）。
        assert_eq!(natural_cmp("7", "07"), Ordering::Less);
        assert_eq!(natural_cmp("07", "007"), Ordering::Less);
        assert_eq!(natural_cmp("7", "007"), Ordering::Less);
        // 等长前导零按字典序。
        assert_eq!(natural_cmp("007", "017"), Ordering::Less);
    }

    // ── 边界:纯数字 vs 数字带后缀/前缀 ───────────────────────────────────────────
    #[test]
    fn digits_then_suffix_and_prefix() {
        assert_eq!(natural_cmp("10", "10a"), Ordering::Less); // 前缀更短更小
        assert_eq!(natural_cmp("10a", "10"), Ordering::Greater);
        assert_eq!(natural_cmp("a10", "a10"), Ordering::Equal);
        // 等长数字段数值相等 → 继续比较其后字符（"v1x" 的 1 == "v1y" 的 1 → 再比 x < y）。
        assert_eq!(natural_cmp("v1x", "v1y"), Ordering::Less);
        // 长度优先:2 位 "01" 段比 1 位 "1" 段长 → "v01x" 更大（不进入"继续"路径,与前导零特征化一致）。
        assert_eq!(natural_cmp("v01x", "v1y"), Ordering::Greater);
    }

    // ── 边界:非数字字符按码点序（与文件树 BINARY / TREE_SORT_KEY 约定一致）──────────
    #[test]
    fn non_digit_uses_codepoint_order() {
        assert_eq!(natural_cmp("a", "b"), Ordering::Less);
        assert_eq!(natural_cmp("Albums", "albums"), Ordering::Less); // 'A'=0x41 < 'a'=0x61
        assert_eq!(natural_cmp("albums", "相册"), Ordering::Less); // ASCII < 多字节 UTF-8
    }

    #[test]
    fn empty_and_prefix_edges() {
        assert_eq!(natural_cmp("", "a"), Ordering::Less);
        assert_eq!(natural_cmp("", ""), Ordering::Equal);
        assert_eq!(natural_cmp("ab", "abc"), Ordering::Less);
        assert_eq!(natural_cmp("abc", "ab"), Ordering::Greater);
    }

    // ── 全序自洽:反对称 + 有序性抽样（含长数字段,防替换引入非全序比较器）──────────────
    #[test]
    fn ordering_is_antisymmetric_and_consistent() {
        let xs = [
            "",
            "a",
            "a1",
            "a01",
            "a1b",
            "a2",
            "a10",
            "a10a",
            "7",
            "07",
            "007",
            "img2",
            "img10",
            "img100",
            "Camera_XHS_170200523737001027401x.jpg",
            "Camera_XHS_170200523981701027401x.jpg",
            "123456789012345678901234567890",
        ];
        for a in xs {
            assert_eq!(natural_cmp(a, a), Ordering::Equal, "自反: {a:?}");
            for b in xs {
                // 反对称:cmp(a,b) == reverse(cmp(b,a))。
                assert_eq!(
                    natural_cmp(a, b),
                    natural_cmp(b, a).reverse(),
                    "反对称: {a:?} vs {b:?}"
                );
            }
        }
        // 有序性:用本比较器排序后,任意相邻对不得逆序（不 panic + 全序足以稳定排序）。
        let mut sorted = xs.to_vec();
        sorted.sort_by(|a, b| natural_cmp(a, b));
        for w in sorted.windows(2) {
            assert_ne!(
                natural_cmp(w[0], w[1]),
                Ordering::Greater,
                "有序: {:?} !> {:?}",
                w[0],
                w[1]
            );
        }
    }

    // ── 对拍基准:非溢出输入下与 lexicmp::natural_cmp 逐项等价（characterization）──────
    // lexicmp 为 [dev-dependencies]（仅测试编译）。生成器保证「数字段 ≤15 位且互不相邻」→ 任何
    // 连续数字段 < u64 溢出阈（~20 位）,基准自身不 panic;据此证明「替换未改动既有排序序」。

    /// 确定性 LCG（Numerical Recipes 常数;避免引入 rand,亦不依赖不可用的运行期随机源）。
    fn lcg_next(state: &mut u64) -> u64 {
        *state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *state >> 33
    }

    /// 生成随机测试串:字母/分隔 token 与数字 token 交替,数字段 ≤15 位且**永不相邻**
    /// （`last_was_digit` 门控）→ 单个连续数字段最长 15 位,严格 < 溢出阈,lexicmp 基准不会 panic。
    fn gen_string(state: &mut u64) -> String {
        let alphabet = ['a', 'b', 'Z', '_', '-', '.', '相'];
        let parts = (lcg_next(state) % 5) + 1;
        let mut s = String::new();
        let mut last_was_digit = false;
        for _ in 0..parts {
            let want_digit = lcg_next(state).is_multiple_of(2) && !last_was_digit;
            if want_digit {
                let len = (lcg_next(state) % 15) + 1; // 1..=15 位
                for _ in 0..len {
                    s.push((b'0' + (lcg_next(state) % 10) as u8) as char);
                }
                last_was_digit = true;
            } else {
                let len = (lcg_next(state) % 4) + 1;
                for _ in 0..len {
                    s.push(alphabet[(lcg_next(state) % alphabet.len() as u64) as usize]);
                }
                last_was_digit = false;
            }
        }
        s
    }

    #[test]
    fn parity_with_lexicmp_on_non_overflow_inputs() {
        let mut state: u64 = 0x1234_5678_9abc_def0;
        for _ in 0..3000 {
            let a = gen_string(&mut state);
            let b = gen_string(&mut state);
            assert_eq!(
                natural_cmp(&a, &b),
                lexicmp::natural_cmp(&a, &b),
                "与 lexicmp 基准不符: {a:?} vs {b:?}"
            );
        }
    }
}
