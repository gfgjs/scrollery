//! 设置值在两个表示之间的唯一转换点:标量/结构值的**规范文本** ↔ TOML 节点、规范 JSON 文本。
//!
//! # 为什么需要这一层
//! 「规范文本」是跨进程契约(IPC 参数与返回值、ConfigManager 内存值表、schema 默认值三者同型),
//! 但对结构类设置(字符串数组/内联对象/映射/对象数组)它必须是 JSON 文本——前端只会拼 JSON,而
//! 磁盘上则要求**原生 TOML 数组/内联表**(方案 §4:不得在文件里嵌套 JSON 字符串)。两侧形态不同,
//! 若让各调用点自行转换,就会出现「同一份值有 N 种合法写法」的比较/相等性灾难(例如
//! {x=0,y=0} 与 {"x":0,"y":0} 被判为「变了」)。故所有转换集中在此,单一实现、单一出口。
//!
//! # 三组函数
//! - text_to_canonical:IPC 传入的原始文本 → 规范文本(严格:未知字段/未知键/越界一律报错)。
//! - item_to_canonical:磁盘 TOML 节点 → 规范文本(宽容:结构内的未知 ID/表外 label 只忽略该
//!   条目并回报,整键仍生效——与「单键类型错只警告、其余键照常」的既有容错姿态一致)。
//! - canonical_to_item / canonical_to_toml_literal:规范文本 → TOML 节点/字面量源码
//!   (写盘与模板渲染共用)。

use serde_json::{Map as JsonMap, Value as Json};
use toml_edit::Item;

use super::schema::{FieldType, SettingDef, SettingKind, StructDef};

/// 画廊底色「跟随界面底面」的字面量(与前端 `themes/types.ts` 的 `GALLERY_AUTO` 同值)。
const GALLERY_AUTO: &str = "auto";

/// 规范颜色判据:小写 `#rrggbb` 六位。**只认这一种写法**——`#rgb`/大写/无 `#` 由前端的
/// `normalizeHex` 在进入 IPC 前收敛(方案 §3:字符串在输入／设置边界统一校验),内核不再
/// 重复实现一套宽松解析,避免同一份颜色有两种合法文本导致比较/相等性分叉。
/// 用户手改 config.toml 时写别的写法会得到一条「应为规范 #rrggbb」的键级警告。
fn is_canonical_hex(text: &str) -> bool {
    let mut chars = text.chars();
    if chars.next() != Some('#') {
        return false;
    }
    let digits: Vec<char> = chars.collect();
    digits.len() == 6
        && digits
            .iter()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(c))
}

/// 颜色字段的取值文本校验;返回规范文本(即其自身)。
fn color_text(field: &str, allow_auto: bool, raw: &str) -> Result<String, String> {
    if allow_auto && raw == GALLERY_AUTO {
        return Ok(GALLERY_AUTO.to_string());
    }
    if is_canonical_hex(raw) {
        return Ok(raw.to_string());
    }
    Err(if allow_auto {
        format!("{field} 应为规范小写 #rrggbb 颜色或 auto")
    } else {
        format!("{field} 应为规范小写 #rrggbb 颜色")
    })
}

/// 从 TOML 节点得到的规范文本 + 被忽略的条目说明(结构内的未知 ID / 表外 label)。
pub struct FromItem {
    /// 规范文本(与 SettingDef::default 同型)。
    pub text: String,
    /// 被忽略的条目(仅告警用;空表示全部条目都生效)。
    pub ignored: Vec<String>,
}

/// 规范小数文本:Rust Debug 已是「最短往返表示」,但它对 Infinity/NaN 会产出 TOML 不接受的
/// 词形,故本函数只应喂入已通过有限性校验的值(调用点已保证),此处再兜一层断言式兜底。
fn float_text(v: f64) -> String {
    debug_assert!(v.is_finite(), "浮点规范文本只接受有限值");
    let s = format!("{v:?}");
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{s}.0")
    }
}

/// JSON 字符串字面量。JSON 的转义规则是 TOML 基本字符串转义的子集(反斜杠、双引号、\n、\r、
/// \t、\uXXXX 两侧都接受),故同一段文本可直接用在 JSON 与 TOML 两处。
fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

// ── 单字段校验 ────────────────────────────────────────────────────────────────

/// 按字段类型校验一个 JSON 值,通过则返回其规范文本。
fn field_text(field: &str, ty: FieldType, value: &Json) -> Result<String, String> {
    match ty {
        FieldType::Bool => value
            .as_bool()
            .map(|b| b.to_string())
            .ok_or_else(|| format!("{field} 应为布尔值")),
        FieldType::Int { min, max } => {
            let n = value.as_i64().ok_or_else(|| format!("{field} 应为整数"))?;
            if n < min || n > max {
                return Err(format!("{field} 应在 {min}–{max} 之间,实际 {n}"));
            }
            Ok(n.to_string())
        }
        FieldType::Float { min, max } => {
            let n = value.as_f64().ok_or_else(|| format!("{field} 应为数值"))?;
            if !n.is_finite() {
                return Err(format!("{field} 不能是无穷值或 NaN"));
            }
            if n < min || n > max {
                return Err(format!("{field} 应在 {min}–{max} 之间,实际 {n}"));
            }
            Ok(float_text(n))
        }
        FieldType::Str => value
            .as_str()
            // 必须带上 JSON 引号:结构类键在 IPC 与内存里都是**规范 JSON 文本**,规范值随后还要被
            // canonical_to_item / canonical_to_toml_literal 重新 parse_json —— 裸串会让整份文本不
            // 是合法 JSON(报「不是合法 JSON 文本:语法错误」),字符串字段的列表/映射因此整体不可用。
            .map(json_string)
            .ok_or_else(|| format!("{field} 应为字符串")),
        FieldType::Enum(options) => {
            let s = value
                .as_str()
                .ok_or_else(|| format!("{field} 应为字符串"))?;
            if options.contains(&s) {
                // 同 Str:枚举也是 JSON 字符串值。
                Ok(json_string(s))
            } else {
                Err(format!(
                    "{field} 取值 {s} 不在允许范围内,允许值:{}",
                    options.join(" / ")
                ))
            }
        }
        FieldType::StrList => str_list_text(value).map_err(|e| format!("{field} {e}")),
        FieldType::Color { allow_auto } => {
            let s = value
                .as_str()
                .ok_or_else(|| format!("{field} 应为字符串"))?;
            // 颜色是字符串值:规范文本里必须带 JSON 引号(同 FieldType::Str 的说明)。
            let checked = color_text(field, allow_auto, s)?;
            Ok(json_string(&checked))
        }
    }
}

/// 固定字段集的结构体 → 规范 JSON 文本。要求字段齐备且无多余字段(结构类值在 IPC 侧是完整对象,
/// 半成品对象应被拒绝而不是静默补默认值——否则前端漏传字段会被当成用户改了那一项)。
fn struct_text(def: &StructDef, value: &Json) -> Result<String, String> {
    let obj = value.as_object().ok_or_else(|| "应为对象".to_string())?;
    for key in obj.keys() {
        if def.field(key).is_none() {
            return Err(format!("出现未知字段 {key}"));
        }
    }
    let mut out = String::from("{");
    for (i, field) in def.fields.iter().enumerate() {
        let v = obj
            .get(field.name)
            .ok_or_else(|| format!("缺少字段 {}", field.name))?;
        if i > 0 {
            out.push(',');
        }
        out.push_str(&json_string(field.name));
        out.push(':');
        out.push_str(&field_text(field.name, field.ty, v)?);
    }
    out.push('}');
    Ok(out)
}

/// 数组内的唯一性/非空约束(见 `schema::UniqueField`)。`entries` 是各条目的**规范 JSON
/// 对象**(提交路径来自 IPC、加载路径来自磁盘,两侧同一份判据)。
///
/// 名称类字段(`is_name`)比较前去除首尾空白(「 暖纸 」与「暖纸」是同一个名字,不按名称自动
/// 覆盖),且去空白后不得为空;ID 类字段要求非空且自身不含首尾空白,并按原值精确比较(放宽会让
/// 「 a 」与「a」在后端成为两条记录、在前端却塌成同一条,见 `schema::UniqueField`)。
/// 返回**第一条**违规的面向用户说明,由调用方决定是整批拒绝(提交)还是丢弃该条目并告警(加载)。
fn check_unique(def: &StructDef, entries: &[Json]) -> Result<(), String> {
    if def.unique.is_empty() {
        return Ok(());
    }
    let mut seen: Vec<(&str, String)> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        for rule in def.unique {
            let Some(raw) = entry.get(rule.name).and_then(Json::as_str) else {
                // 字段缺失/类型不符已由字段级校验报错,此处不重复报。
                continue;
            };
            let key = if rule.is_name {
                raw.trim().to_string()
            } else {
                if raw.is_empty() || raw.trim() != raw {
                    return Err(format!(
                        "第 {} 条的 {} 不能为空或含首尾空白(ID 是稳定定位键,原样保存)",
                        index + 1,
                        rule.name
                    ));
                }
                raw.to_string()
            };
            if rule.is_name && key.is_empty() {
                return Err(format!("第 {} 条的名称去首尾空白后为空", index + 1));
            }
            if seen.iter().any(|(name, v)| *name == rule.name && *v == key) {
                return Err(format!(
                    "第 {} 条的 {} 与前面的条目重复({key}):{}须唯一",
                    index + 1,
                    rule.name,
                    if rule.is_name {
                        "名称去首尾空白后"
                    } else {
                        ""
                    }
                ));
            }
            seen.push((rule.name, key));
        }
    }
    Ok(())
}

// ── IPC 文本 → 规范文本(严格)────────────────────────────────────────────────

/// 把 IPC 传入的原始文本转成规范文本。标量类值即其字面文本,结构类值是规范 JSON 文本。
///
/// 严格口径(方案 §5.1):结构里出现未知字段/未知 ID/表外 label、数值越界、类型不符一律报错,
/// 由调用方整批拒绝;不做「猜用户意图」的补默认值。
pub fn text_to_canonical(def: &SettingDef, raw: &str) -> Result<String, String> {
    let kind = def.kind;
    match kind {
        SettingKind::Bool => match raw {
            "true" => Ok("true".to_string()),
            "false" => Ok("false".to_string()),
            _ => Err("应为 true 或 false".to_string()),
        },
        SettingKind::UInt => raw
            .parse::<u64>()
            .map(|n| n.to_string())
            .map_err(|_| "应为非负整数".to_string()),
        SettingKind::Int { min, max } => {
            let n = raw.parse::<i64>().map_err(|_| "应为整数".to_string())?;
            if n < min || n > max {
                return Err(format!("应在 {min}–{max} 之间,实际 {n}"));
            }
            Ok(n.to_string())
        }
        SettingKind::Float { min, max } => {
            let n = raw.parse::<f64>().map_err(|_| "应为数值".to_string())?;
            if !n.is_finite() {
                return Err("不能是无穷值或 NaN".to_string());
            }
            if n < min || n > max {
                return Err(format!("应在 {min}–{max} 之间,实际 {n}"));
            }
            Ok(float_text(n))
        }
        SettingKind::Str | SettingKind::Path => Ok(raw.to_string()),
        SettingKind::Enum(options) => {
            if options.contains(&raw) {
                Ok(raw.to_string())
            } else {
                Err(format!(
                    "取值 {raw} 不在允许范围内,允许值:{}",
                    options.join(" / ")
                ))
            }
        }
        SettingKind::StrList => Ok(str_list_text(&parse_json(raw)?)?),
        SettingKind::Struct(def) => struct_text(def, &parse_json(raw)?),
        SettingKind::BoolMap { ids } => {
            let parsed = parse_json(raw)?;
            let obj = parsed
                .as_object()
                .ok_or_else(|| "应为 名称 → 布尔值 的映射".to_string())?;
            for key in obj.keys() {
                if !ids.contains(&key.as_str()) {
                    return Err(format!("出现未知 ID {key}"));
                }
            }
            let mut out = String::from("{");
            for (i, id) in ids.iter().enumerate() {
                let v = obj
                    .get(*id)
                    .ok_or_else(|| format!("缺少 ID {id}（提交结构类值须给完整映射）"))?
                    .as_bool()
                    .ok_or_else(|| format!("ID {id} 应为布尔值"))?;
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&json_string(id));
                out.push(':');
                out.push_str(if v { "true" } else { "false" });
            }
            out.push('}');
            Ok(out)
        }
        SettingKind::StructMap { labels, value } => {
            let parsed = parse_json(raw)?;
            let obj = parsed
                .as_object()
                .ok_or_else(|| "应为 名称 → 结构 的映射".to_string())?;
            for key in obj.keys() {
                if !labels.contains(&key.as_str()) {
                    return Err(format!("出现未登记的窗口标签 {key}"));
                }
            }
            emit_struct_map(obj, value)
        }
        SettingKind::StructList(def) => {
            let parsed = parse_json(raw)?;
            let arr = parsed
                .as_array()
                .ok_or_else(|| "应为结构数组".to_string())?;
            // 整批拒绝口径:ID/名称去重与空名称在此拦下,不做「丢掉坏条目、写入其余」的部分提交。
            check_unique(def, arr)?;
            let mut out = String::from("[");
            for (i, item) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&struct_text(def, item)?);
            }
            out.push(']');
            Ok(out)
        }
    }
}

/// 字符串数组的规范文本。非字符串元素即报错。
fn str_list_text(value: &Json) -> Result<String, String> {
    let arr = value
        .as_array()
        .ok_or_else(|| "应为字符串数组".to_string())?;
    let mut out = String::from("[");
    for (i, item) in arr.iter().enumerate() {
        let s = item
            .as_str()
            .ok_or_else(|| "数组元素应为字符串".to_string())?;
        if i > 0 {
            out.push(',');
        }
        out.push_str(&json_string(s));
    }
    out.push(']');
    Ok(out)
}

fn parse_json(raw: &str) -> Result<Json, String> {
    serde_json::from_str(raw).map_err(|e| {
        use serde_json::error::Category;
        // 只报「哪一类错」:原始解析器文本对本场景无用,且可能内嵌用户输入片段。
        let kind = match e.classify() {
            Category::Io => "读取失败",
            Category::Syntax => "语法错误",
            Category::Data => "结构不符",
            Category::Eof => "内容不完整",
        };
        format!("不是合法 JSON 文本:{kind}")
    })
}

/// 把 名称 → 结构 的映射渲染成规范 JSON 文本。键按字典序排列,使同一份内容只有一种文本形态
/// (前端拖拽改序不会产生假变更,事件比对也不会因顺序抖动而误报)。
fn emit_struct_map(obj: &JsonMap<String, Json>, value_def: &StructDef) -> Result<String, String> {
    let mut sorted: Vec<&String> = obj.keys().collect();
    sorted.sort();
    let mut out = String::from("{");
    for (i, key) in sorted.iter().enumerate() {
        let v = obj
            .get(key.as_str())
            .ok_or_else(|| format!("缺少 {key} 的取值"))?;
        if i > 0 {
            out.push(',');
        }
        out.push_str(&json_string(key));
        out.push(':');
        out.push_str(&struct_text(value_def, v)?);
    }
    out.push('}');
    Ok(out)
}

// ── TOML 节点 → 规范文本(宽容)──────────────────────────────────────────────

/// 把磁盘上的 TOML 节点转成规范文本。**宽容口径**:固定 ID 映射里缺的 ID 用该键 schema 默认值
/// 补齐(前端因此总能拿到完整映射,不必自己判缺省),结构内的未知 ID、表外窗口 label、结构里多余
/// 的字段只忽略该条目并记入 ignored —— 一个陌生条目不该让整份配置失效(方案 §5.4)。
/// 类型不符(如布尔键写成字符串)仍返回 Err,由调用方记为该键的警告并跳过。
pub fn item_to_canonical(def: &SettingDef, item: &Item) -> Result<FromItem, String> {
    let mut ignored = Vec::new();
    let text = match def.kind {
        SettingKind::Bool => item
            .as_bool()
            .map(|b| b.to_string())
            .ok_or_else(|| "应为布尔值 true 或 false".to_string())?,
        SettingKind::UInt => item
            .as_integer()
            .filter(|n| *n >= 0)
            .map(|n| n.to_string())
            .ok_or_else(|| "应为非负整数".to_string())?,
        SettingKind::Int { min, max } => {
            let n = item.as_integer().ok_or_else(|| "应为整数".to_string())?;
            if n < min || n > max {
                return Err(format!("应在 {min}–{max} 之间,实际 {n}"));
            }
            n.to_string()
        }
        SettingKind::Float { min, max } => {
            let n = item
                .as_float()
                .or_else(|| item.as_integer().map(|i| i as f64))
                .ok_or_else(|| "应为数值(小数)".to_string())?;
            if !n.is_finite() {
                return Err("不能是无穷值或 NaN".to_string());
            }
            if n < min || n > max {
                return Err(format!("应在 {min}–{max} 之间,实际 {n}"));
            }
            float_text(n)
        }
        SettingKind::Str | SettingKind::Path => item
            .as_str()
            .map(std::string::ToString::to_string)
            .ok_or_else(|| "应为字符串(用双引号包裹)".to_string())?,
        SettingKind::Enum(options) => {
            let s = item
                .as_str()
                .ok_or_else(|| "应为字符串(用双引号包裹)".to_string())?;
            if !options.contains(&s) {
                return Err(format!(
                    "取值 {s} 不在允许范围内,允许值:{}",
                    options.join(" / ")
                ));
            }
            s.to_string()
        }
        SettingKind::StrList => {
            let arr = item
                .as_array()
                .ok_or_else(|| "应为字符串数组".to_string())?;
            let mut json = Vec::new();
            for v in arr.iter() {
                json.push(Json::String(
                    v.as_str()
                        .ok_or_else(|| "数组元素应为字符串".to_string())?
                        .to_string(),
                ));
            }
            str_list_text(&Json::Array(json))?
        }
        SettingKind::Struct(def_struct) => {
            struct_text(def_struct, &struct_to_json(def_struct, item, &mut ignored)?)?
        }
        SettingKind::BoolMap { ids } => {
            let mut out = String::from("{");
            for (i, id) in ids.iter().enumerate() {
                let v = table_get(item, id)
                    .and_then(|it| it.as_bool())
                    .unwrap_or_else(|| bool_map_default(def, id));
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&json_string(id));
                out.push(':');
                out.push_str(if v { "true" } else { "false" });
            }
            out.push('}');
            for (key, _) in table_entries(item)? {
                if !ids.contains(&key.as_str()) {
                    ignored.push(format!("未知 ID {key}（已忽略）"));
                }
            }
            out
        }
        SettingKind::StructMap { labels, value } => {
            let mut json = JsonMap::new();
            for (key, entry) in table_entries(item)? {
                if !labels.contains(&key.as_str()) {
                    ignored.push(format!("未登记的窗口标签 {key}（已忽略）"));
                    continue;
                }
                json.insert(key, struct_to_json(value, &entry, &mut ignored)?);
            }
            emit_struct_map(&json, value)?
        }
        SettingKind::StructList(def_struct) => {
            let arr = item
                .as_array()
                .ok_or_else(|| "应为结构数组(TOML 表数组)".to_string())?;
            // 先逐条转成规范 JSON 对象:唯一性判据与提交路径共用同一份比较口径,故在拿到对象
            // 之后再判,而不是在 TOML 节点上另写一套。
            let mut objects = Vec::with_capacity(arr.len());
            for (i, v) in arr.iter().enumerate() {
                let mut entry_ignored = Vec::new();
                let obj = struct_to_json(def_struct, &Item::Value(v.clone()), &mut entry_ignored)?;
                // 加载是宽容路径:坏条目只丢掉自己并回报一条警告,不让整键失效(方案 §5.4)。
                if let Err(reason) = check_unique(
                    def_struct,
                    &objects.iter().chain([&obj]).cloned().collect::<Vec<_>>(),
                ) {
                    ignored.push(format!("第 {} 条已忽略:{reason}", i + 1));
                    continue;
                }
                // 忽略说明要带上条目序号,否则用户不知道是哪一条缺字段/多了字段。
                for note in entry_ignored {
                    ignored.push(format!("第 {} 条 {note}", i + 1));
                }
                objects.push(obj);
            }
            let mut out = String::from("[");
            for (i, obj) in objects.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&struct_text(def_struct, obj)?);
            }
            out.push(']');
            out
        }
    };
    Ok(FromItem { text, ignored })
}

/// BoolMap 缺项时取该键 schema 默认文本里的同 ID 值。默认值本身一定是完整映射(有单测锁住),
/// 解析失败或取不到时回落 false(不 panic)。
fn bool_map_default(def: &SettingDef, id: &str) -> bool {
    serde_json::from_str::<Json>(def.default)
        .ok()
        .and_then(|j| j.get(id).and_then(Json::as_bool))
        .unwrap_or(false)
}

/// TOML 结构节点 → JSON 对象。缺字段用该字段类型的零值补齐并告警:文件里手写半份结构属笔误,
/// 若因此让整键失效会更糟(方案 §5.4 的宽容姿态)。
fn struct_to_json(def: &StructDef, item: &Item, ignored: &mut Vec<String>) -> Result<Json, String> {
    if table_entries(item).is_err() {
        return Err("应为内联表 { ... }".to_string());
    }
    let mut obj = JsonMap::new();
    for field in def.fields {
        match table_get(item, field.name) {
            Some(entry) => {
                obj.insert(
                    field.name.to_string(),
                    item_value_to_json(field.ty, &entry)
                        .map_err(|e| format!("字段 {} {e}", field.name))?,
                );
            }
            None => {
                let Some(fallback) = zero_json(field.ty) else {
                    return Err(format!("缺少字段 {}", field.name));
                };
                ignored.push(format!("缺少字段 {}（已按默认值补齐）", field.name));
                obj.insert(field.name.to_string(), fallback);
            }
        }
    }
    for (key, _) in table_entries(item)? {
        if def.field(&key).is_none() {
            ignored.push(format!("未知字段 {key}（已忽略）"));
        }
    }
    Ok(Json::Object(obj))
}

/// TOML 结构里缺字段时的补齐值;`None` 表示该字段**没有合法零值**,缺失即非法。
///
/// 颜色字段正是后者:非 auto 的颜色没有「空」这种合法取值,照旧补 `""` 会让后续字段级校验判它
/// 非法、进而让**整个键**失效(结构类加载的既有容错是「缺字段补齐 + 告警」,不是整键失效),
/// 与「一个坏条目只丢自己」的口径冲突。改由此处直接报缺字段,把失败停在条目粒度。
fn zero_json(ty: FieldType) -> Option<Json> {
    Some(match ty {
        FieldType::Bool => Json::Bool(false),
        FieldType::Int { .. } => Json::from(0i64),
        FieldType::Float { .. } => Json::from(0.0f64),
        FieldType::Str => Json::String(String::new()),
        FieldType::Enum(options) => {
            Json::String(options.first().copied().unwrap_or("").to_string())
        }
        FieldType::StrList => Json::Array(Vec::new()),
        FieldType::Color { allow_auto } => {
            if !allow_auto {
                return None;
            }
            Json::String(GALLERY_AUTO.to_string())
        }
    })
}

/// TOML 值 → JSON 值(按字段类型校验,类型不符即报错)。
fn item_value_to_json(ty: FieldType, item: &Item) -> Result<Json, String> {
    Ok(match ty {
        FieldType::Bool => Json::Bool(item.as_bool().ok_or_else(|| "应为布尔值".to_string())?),
        FieldType::Int { min, max } => {
            let n = item.as_integer().ok_or_else(|| "应为整数".to_string())?;
            if n < min || n > max {
                return Err(format!("应在 {min}–{max} 之间,实际 {n}"));
            }
            Json::from(n)
        }
        FieldType::Float { min, max } => {
            let n = item
                .as_float()
                .or_else(|| item.as_integer().map(|i| i as f64))
                .ok_or_else(|| "应为数值".to_string())?;
            if !n.is_finite() {
                return Err("不能是无穷值或 NaN".to_string());
            }
            if n < min || n > max {
                return Err(format!("应在 {min}–{max} 之间,实际 {n}"));
            }
            Json::from(n)
        }
        FieldType::Str => Json::String(
            item.as_str()
                .ok_or_else(|| "应为字符串".to_string())?
                .to_string(),
        ),
        FieldType::Enum(options) => {
            let s = item.as_str().ok_or_else(|| "应为字符串".to_string())?;
            if !options.contains(&s) {
                return Err(format!("取值 {s} 不在允许范围内"));
            }
            Json::String(s.to_string())
        }
        FieldType::StrList => {
            let arr = item
                .as_array()
                .ok_or_else(|| "应为字符串数组".to_string())?;
            let mut out = Vec::new();
            for v in arr.iter() {
                out.push(Json::String(
                    v.as_str()
                        .ok_or_else(|| "数组元素应为字符串".to_string())?
                        .to_string(),
                ));
            }
            Json::Array(out)
        }
        FieldType::Color { allow_auto } => {
            let s = item
                .as_str()
                .ok_or_else(|| "应为字符串(颜色用双引号包裹)".to_string())?;
            Json::String(color_text("颜色", allow_auto, s)?)
        }
    })
}

// ── TOML 表读取小工具(内联表与常规表统一)────────────────────────────────────

fn table_get(item: &Item, key: &str) -> Option<Item> {
    if let Some(t) = item.as_inline_table() {
        return t.get(key).map(|v| Item::Value(v.clone()));
    }
    item.as_table().and_then(|t| t.get(key).cloned())
}

fn table_entries(item: &Item) -> Result<Vec<(String, Item)>, String> {
    if let Some(t) = item.as_inline_table() {
        return Ok(t
            .iter()
            .map(|(k, v)| (k.to_string(), Item::Value(v.clone())))
            .collect());
    }
    if let Some(t) = item.as_table() {
        return Ok(t.iter().map(|(k, v)| (k.to_string(), v.clone())).collect());
    }
    Err("应为内联表 { ... }".to_string())
}

// ── 规范文本 → TOML ─────────────────────────────────────────────────────────

/// 规范文本 → TOML 字面量源码。结构类值渲染成**原生 TOML 数组/内联表**(而不是 JSON 字符串),
/// 满足「磁盘必须原生 TOML」的硬要求;字符串类按 TOML 基本字符串转义。
pub fn canonical_to_toml_literal(def: &SettingDef, canonical: &str) -> Result<String, String> {
    Ok(match def.kind {
        SettingKind::Bool | SettingKind::UInt | SettingKind::Int { .. } => canonical.to_string(),
        SettingKind::Float { .. } => float_text(
            canonical
                .parse::<f64>()
                .map_err(|_| "小数规范文本无法解析".to_string())?,
        ),
        SettingKind::Str | SettingKind::Path | SettingKind::Enum(_) => json_string(canonical),
        SettingKind::StrList
        | SettingKind::Struct(_)
        | SettingKind::BoolMap { .. }
        | SettingKind::StructMap { .. }
        | SettingKind::StructList(_) => json_to_toml_literal(&parse_json(canonical)?)?,
    })
}

/// JSON 值 → TOML 字面量源码(对象一律渲染成内联表,数组一律渲染成 TOML 数组)。
fn json_to_toml_literal(j: &Json) -> Result<String, String> {
    Ok(match j {
        Json::Null => return Err("不支持 null 值".to_string()),
        Json::Bool(b) => b.to_string(),
        Json::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(f) = n.as_f64() {
                float_text(f)
            } else {
                return Err("数值超出可表示范围".to_string());
            }
        }
        Json::String(s) => json_string(s),
        Json::Array(items) => {
            let mut out = String::from("[");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&json_to_toml_literal(item)?);
            }
            out.push(']');
            out
        }
        Json::Object(map) => {
            let mut out = String::from("{ ");
            for (i, (k, v)) in map.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&json_string(k));
                out.push_str(" = ");
                out.push_str(&json_to_toml_literal(v)?);
            }
            out.push_str(" }");
            out
        }
    })
}

/// 规范文本 → 可写入文档的 TOML 节点。**经字面量源码 + 单键文档解析**构造,而不是手工拼
/// Value:手工构造路径对「整数值小数」的再渲染行为依赖上游私有实现(crate 内部的
/// Formatted/Repr 不对外开放),一旦产出 0 而非 0.0,用户文件里就会出现
/// selection_bar_offset = { x = 0 } 这种整数冒充小数的写法。走解析路径则写盘文本与加载校验
/// 共用同一套规则,不会分叉。
pub fn canonical_to_item(def: &SettingDef, canonical: &str) -> Result<Item, String> {
    let literal = canonical_to_toml_literal(def, canonical)?;
    let doc = format!("v = {literal}\n")
        .parse::<toml_edit::DocumentMut>()
        .map_err(|_| "规范文本无法渲染为 TOML 字面量".to_string())?;
    doc.get("v")
        .cloned()
        .ok_or_else(|| "TOML 字面量解析结果为空".to_string())
}

/// 规范字符串数组文本 → TOML 数组节点(供模板渲染复用)。
/// 结构映射类键里**单条目标签**的合并:在 `current`(规范 JSON 文本)基础上把 `label` 设为
/// `label_json`,其余标签原样保留,返回新的规范文本。
///
/// 存在的意义是并发正确性:窗口几何由多个窗口各自提交,若由调用方「读整表 → 改一项 → 写整表」,
/// 两个窗口的提交会互相覆盖。合并动作放在配置写锁内做,就不会丢其他窗口刚写的标签。
pub fn struct_map_upsert(
    def: &SettingDef,
    current: &str,
    label: &str,
    label_json: &str,
) -> Result<String, crate::config::ConfigFileError> {
    use crate::config::ConfigFileError;
    let SettingKind::StructMap {
        labels,
        value: value_def,
    } = def.kind
    else {
        return Err(ConfigFileError::InvalidValue(format!(
            "{} 不是结构映射类设置,不支持单标签合并",
            def.key
        )));
    };
    if !labels.contains(&label) {
        return Err(ConfigFileError::InvalidValue(format!(
            "未登记的窗口标签 {label}"
        )));
    }
    let mut map: JsonMap<String, Json> = serde_json::from_str(current)
        .map_err(|_| ConfigFileError::InvalidValue("当前值不是合法的映射文本".to_string()))?;
    let entry: Json = parse_json(label_json).map_err(ConfigFileError::InvalidValue)?;
    // 先按结构定义规范化一次:非法结构在此拦下,不会写进文件。
    struct_text(value_def, &entry).map_err(ConfigFileError::InvalidValue)?;
    map.insert(label.to_string(), entry);
    emit_struct_map(&map, value_def).map_err(ConfigFileError::InvalidValue)
}
