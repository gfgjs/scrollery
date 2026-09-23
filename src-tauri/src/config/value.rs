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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{SETTING_DEFS, THEME_SEED, WINDOW_GEOMETRY, WINDOW_LABELS};

    fn def(key: &str) -> &'static SettingDef {
        SettingDef::find(key).unwrap_or_else(|| panic!("schema 缺少键 {key}"))
    }

    fn parse_item(text: &str) -> Item {
        format!("v = {text}\n")
            .parse::<toml_edit::DocumentMut>()
            .unwrap()
            .get("v")
            .unwrap()
            .clone()
    }

    const MAIN_GEOMETRY: &str = "{\"x\":-100.5,\"y\":0.0,\"width\":1280.0,\"height\":820.0,\"scaleFactor\":1.25,\"maximized\":false}";

    /// 每个 schema 默认值都必须已经是规范文本:否则「默认值」与「用户提交值」两套口径会让首次
    /// 加载就产生假变更(空表 → 默认值 的 diff 永远非空)。这是整个转换层的地基不变量。
    #[test]
    fn every_schema_default_is_already_canonical() {
        for d in SETTING_DEFS {
            let canonical = text_to_canonical(d, d.default)
                .unwrap_or_else(|e| panic!("{} 的默认值不是规范文本:{e}", d.key));
            assert_eq!(
                canonical, d.default,
                "{} 的默认值经规范化后与自身不等",
                d.key
            );
        }
    }

    /// 每个 schema 默认值都能渲染成合法 TOML 字面量并可被原样读回(模板渲染与原子写盘都依赖它)。
    #[test]
    fn every_schema_default_roundtrips_through_toml() {
        for d in SETTING_DEFS {
            let item = canonical_to_item(d, d.default)
                .unwrap_or_else(|e| panic!("{} 渲染 TOML 失败:{e}", d.key));
            let back =
                item_to_canonical(d, &item).unwrap_or_else(|e| panic!("{} 读回失败:{e}", d.key));
            assert_eq!(back.text, d.default, "{} 经 TOML 往返后不等", d.key);
        }
    }

    /// 每个键的默认值渲染出的模板行必须是「注释掉也不报错」的合法字面量(读回自身相等即可),
    /// 且不会退化成 JSON 字符串形态(结构类键在文件里必须是原生 TOML)。
    #[test]
    fn structured_defaults_render_as_native_toml_not_json_string() {
        for d in SETTING_DEFS {
            let is_structured = matches!(
                d.kind,
                SettingKind::StrList
                    | SettingKind::Struct(_)
                    | SettingKind::BoolMap { .. }
                    | SettingKind::StructMap { .. }
                    | SettingKind::StructList(_)
            );
            if !is_structured {
                continue;
            }
            let literal = canonical_to_toml_literal(d, d.default).unwrap();
            assert!(
                !literal.starts_with('"'),
                "{} 被渲染成了 JSON 字符串而非原生 TOML:{literal}",
                d.key
            );
        }
    }

    /// 小数不被截断:0 / 1.75 / 负值都保持小数形态,且 0 写成 0.0 而不是冒充整数。
    #[test]
    fn float_values_keep_decimal_form() {
        let d = def("player_volume");
        assert_eq!(text_to_canonical(d, "0").unwrap(), "0.0");
        assert_eq!(text_to_canonical(d, "0.75").unwrap(), "0.75");
        assert_eq!(
            canonical_to_toml_literal(d, "0.0").unwrap(),
            "0.0",
            "0 必须写成 0.0,不能在文件里冒充整数"
        );

        let geom = def("window_geometry");
        let negative = "{\"main\":{\"x\":-1920.0,\"y\":-240.5,\"width\":1600.0,\"height\":900.0,\"scaleFactor\":1.5,\"maximized\":false}}";
        let canonical = text_to_canonical(geom, negative).unwrap();
        assert!(canonical.contains("-1920.0"), "{canonical}");
        assert!(canonical.contains("-240.5"), "{canonical}");
    }

    /// 无穷 / NaN 一律拒绝(负坐标合法,但非有限值不合法)。
    #[test]
    fn non_finite_floats_are_rejected() {
        let d = def("player_volume");
        for raw in ["inf", "-inf", "nan", "Infinity", "NaN"] {
            assert!(
                text_to_canonical(d, raw).is_err(),
                "{raw} 不应被接受为合法小数"
            );
        }
        let geom = def("window_geometry");
        let infinite = "{\"main\":{\"x\":1e400,\"y\":0.0,\"width\":1280.0,\"height\":820.0,\"scaleFactor\":1.0,\"maximized\":false}}";
        assert!(text_to_canonical(geom, infinite).is_err());
    }

    /// 数值越界与类型不符被拒绝(音量 0–1、倍速 0.25–3、窗口宽高 ≥ 1)。
    #[test]
    fn out_of_range_and_wrong_type_are_rejected() {
        assert!(text_to_canonical(def("player_volume"), "1.5").is_err());
        assert!(text_to_canonical(def("player_rate"), "0.1").is_err());
        assert!(text_to_canonical(def("player_muted"), "1").is_err());
        let zero_width = "{\"main\":{\"x\":0.0,\"y\":0.0,\"width\":0.0,\"height\":820.0,\"scaleFactor\":1.0,\"maximized\":false}}";
        assert!(text_to_canonical(def("window_geometry"), zero_width).is_err());
    }

    /// 结构对象的未知字段与缺字段都被拒绝(不静默补默认值,否则前端漏传会被当成用户改了那一项)。
    #[test]
    fn struct_patch_rejects_unknown_and_missing_fields() {
        let geom = def("window_geometry");
        let unknown = "{\"main\":{\"x\":0.0,\"y\":0.0,\"width\":1280.0,\"height\":820.0,\"scaleFactor\":1.0,\"maximized\":false,\"extra\":1}}";
        assert!(text_to_canonical(geom, unknown).is_err());
        let missing = "{\"main\":{\"x\":0.0}}";
        assert!(text_to_canonical(geom, missing).is_err());
    }

    /// 表外窗口 label 在 patch 时被拒绝(加载时只忽略)。
    #[test]
    fn struct_map_patch_rejects_unregistered_label() {
        let geom = def("window_geometry");
        let other = format!("{{\"ghost\":{MAIN_GEOMETRY}}}");
        let err = text_to_canonical(geom, &other).expect_err("表外 label 应被拒绝");
        assert!(err.contains("ghost"), "{err}");
        for label in WINDOW_LABELS {
            let ok = format!("{{\"{label}\":{MAIN_GEOMETRY}}}");
            assert!(text_to_canonical(geom, &ok).is_ok(), "{label} 应被接受");
        }
    }

    /// 加载时表外 label 只忽略该条目,登记过的 label 照常生效(不因一个陌生标签让整键失效)。
    #[test]
    fn struct_map_load_ignores_unknown_label_only() {
        let geom = def("window_geometry");
        // ghost 是**表外 label**:本用例要的是「文件里出现未登记 label 时只忽略该条」。
        // 它必须写成合法 TOML —— 文件本身就是 TOML;MAIN_GEOMETRY 是 JSON 常量,只适用于 JSON 文本用例。
        let item = parse_item(&format!(
            "{{ main = {{ x = 10, y = 20, width = 1280, height = 820, scaleFactor = 1.0, maximized = false }}, ghost = {{ x = -100.5, y = 0.0, width = 1280.0, height = 820.0, scaleFactor = 1.25, maximized = false }} }}"
        ));
        let got = item_to_canonical(geom, &item).unwrap();
        assert!(got.text.contains("\"main\""), "{}", got.text);
        assert!(!got.text.contains("ghost"), "表外 label 不应进入生效值");
        assert_eq!(got.ignored.len(), 1, "应回报一条忽略说明:{:?}", got.ignored);
    }

    /// 映射键序按字典序归一:同一份内容的不同书写顺序产出的规范文本必须相同(否则前端拖拽改序
    /// 会被当成「值变了」而触发多余广播与派生重启)。
    #[test]
    fn struct_map_key_order_is_canonical() {
        let geom = def("window_geometry");
        let a = format!("{{\"main\":{MAIN_GEOMETRY},\"logs\":{MAIN_GEOMETRY}}}");
        let b = format!("{{\"logs\":{MAIN_GEOMETRY},\"main\":{MAIN_GEOMETRY}}}");
        assert_eq!(
            text_to_canonical(geom, &a).unwrap(),
            text_to_canonical(geom, &b).unwrap()
        );
    }

    /// BoolMap 加载时补齐缺失 ID(取 schema 默认),并忽略未知 ID、汇报忽略项。
    #[test]
    fn bool_map_load_fills_missing_ids_and_ignores_unknown() {
        let d = def("settings_cards_expanded");
        let item = parse_item("{ danger = true, notACard = false }");
        let got = item_to_canonical(d, &item).unwrap();
        assert!(got.text.contains("\"danger\":true"), "{}", got.text);
        assert!(
            got.text.contains("\"galleryTimeline\":false"),
            "旧配置缺失时间轴分组时应默认收起:{}",
            got.text
        );
        assert!(
            got.text.contains("\"common\":true"),
            "缺失 ID 应补 schema 默认:{}",
            got.text
        );
        assert!(!got.text.contains("notACard"));
        assert_eq!(got.ignored.len(), 1);
    }

    /// BoolMap 的 patch 必须给完整映射,且未知 ID 被拒绝。
    #[test]
    fn bool_map_patch_requires_complete_known_ids() {
        let d = def("reader_panel_expanded");
        assert!(
            text_to_canonical(d, "{\"theme\":true}").is_err(),
            "缺 ID 应被拒绝"
        );
        let complete = "{\"theme\":true,\"typography\":true,\"paging\":false,\"book\":false}";
        assert!(text_to_canonical(d, complete).is_ok());
        assert!(text_to_canonical(d, "{\"theme\":true,\"nope\":true}").is_err());
    }

    /// 结构类字段在 TOML 里写成整数也接受(用户手写 x = 10 是自然写法),但归一为小数文本。
    #[test]
    fn integer_typed_toml_is_accepted_for_float_field() {
        let geom = def("window_geometry");
        let item = parse_item(
            "{ main = { x = 10, y = 20, width = 1280, height = 820, scaleFactor = 1, maximized = false } }",
        );
        let got = item_to_canonical(geom, &item).unwrap();
        assert!(got.text.contains("\"x\":10.0"), "{}", got.text);
        assert!(got.text.contains("\"scaleFactor\":1.0"), "{}", got.text);
    }

    /// 日志预设:对象数组在文件里写成 TOML 表数组,读回为规范 JSON 文本;未知字段只忽略。
    #[test]
    fn log_filter_presets_roundtrip_and_ignore_unknown_field() {
        let d = def("log_filter_presets");
        let raw = "[{\"name\":\"err\",\"levels\":[\"error\",\"warn\"],\"target\":\"scrollery\",\"text\":\"io\",\"textMode\":\"substring\"}]";
        let canonical = text_to_canonical(d, raw).unwrap();
        let item = canonical_to_item(d, &canonical).unwrap();
        let back = item_to_canonical(d, &item).unwrap();
        assert_eq!(back.text, canonical);
        assert!(back.ignored.is_empty());

        let with_extra = parse_item(
            "[{ name = \"err\", levels = [\"error\"], target = \"\", text = \"\", textMode = \"regex\", extra = 1 }]",
        );
        let got = item_to_canonical(d, &with_extra).unwrap();
        assert_eq!(got.ignored.len(), 1, "{:?}", got.ignored);
        assert!(got.text.contains("\"regex\""));
    }

    /// 文本模式的合法值受枚举约束(regex / substring 之外一律拒绝)。
    #[test]
    fn log_filter_preset_text_mode_enum_is_enforced() {
        let d = def("log_filter_presets");
        let bad =
            "[{\"name\":\"x\",\"levels\":[],\"target\":\"\",\"text\":\"\",\"textMode\":\"glob\"}]";
        assert!(text_to_canonical(d, bad).is_err());
    }

    /// 窗口几何结构定义与派工约定逐字段一致(字段名/顺序是窗口模块的接线依据,改名即破坏契约)。
    #[test]
    fn window_geometry_field_contract_is_stable() {
        let names: Vec<&str> = WINDOW_GEOMETRY.fields.iter().map(|f| f.name).collect();
        assert_eq!(
            names,
            vec!["x", "y", "width", "height", "scaleFactor", "maximized"]
        );
    }

    // ── 主题配色(批次 A)─────────────────────────────────────────────────────

    /// 一条合法的个人主题条目(字段齐备,颜色规范、名称已去空白)。
    const SAVED_THEME_JSON: &str = "{\"id\":\"t1\",\"name\":\"暖纸\",\
\"light_background\":\"#ffffff\",\"light_foreground\":\"#202024\",\"light_accent\":\"#087f5b\",\
\"light_contrast\":45,\"light_gallery\":\"auto\",\"dark_background\":\"#181818\",\
\"dark_foreground\":\"#f4f4f5\",\"dark_accent\":\"#34d399\",\"dark_contrast\":60,\
\"dark_gallery\":\"#101010\",\"material\":\"mica\",\"opacity\":90,\"visual_style\":\"forest\"}";

    /// 配色种子的合法/非法边界:规范 #rrggbb 通过;gallery 另接受 auto;非规范写法、越界对比度、
    /// 缺字段、多字段一律拒绝(提交是整批拒绝口径,不做「猜用户意图」的补默认值)。
    #[test]
    fn theme_palette_accepts_only_canonical_colors_and_contrast_range() {
        let light = def("theme_light_palette");
        let ok = "{\"background\":\"#ffffff\",\"foreground\":\"#202024\",\"accent\":\"#087f5b\",\"contrast\":45,\"gallery\":\"auto\"}";
        assert_eq!(text_to_canonical(light, ok).unwrap(), ok);
        assert!(text_to_canonical(light, &ok.replace("45", "0")).is_ok());
        assert!(text_to_canonical(light, &ok.replace("45", "100")).is_ok());

        // 非规范写法:#RGB / #rgb / 大写 / 无 # / 命名色 / 超长,全部拒绝(前端 normalizeHex 负责收敛)。
        for bad in [
            "#FFF", "#ffffff ", "#FFFFFF", "ffffff", "white", "#fffffff", "#fffff",
        ] {
            let raw = ok.replace("#ffffff", bad);
            assert!(text_to_canonical(light, &raw).is_err(), "{bad} 不应被接受");
        }

        // 对比度越界与类型不符。
        assert!(text_to_canonical(light, &ok.replace("45", "101")).is_err());
        assert!(text_to_canonical(light, &ok.replace("45", "-1")).is_err());
        assert!(text_to_canonical(light, &ok.replace("45", "\"45\"")).is_err());
        // gallery 只接受 auto 或规范颜色,且是字符串。
        assert!(text_to_canonical(light, &ok.replace("auto", "#abcdef")).is_ok());
        assert!(text_to_canonical(light, &ok.replace("auto", "AUTO")).is_err());
        // 缺字段 / 多字段。
        assert!(text_to_canonical(light, "{\"background\":\"#ffffff\"}").is_err());
        let extra = ok.replace("}", ",\"extra\":1}");
        assert!(text_to_canonical(light, &extra).is_err());
    }

    /// 个人主题在设置边界的两条硬约束:ID 必须非空且不含首尾空白(放宽会让同一条记录在后端是两条、
    /// 在前端塌成一条),名称去首尾空白后不得为空、不得与已有条目重名;两者都在提交时整批拒绝。
    #[test]
    fn saved_themes_enforce_strict_id_and_trimmed_unique_names() {
        let key = def("theme_saved_themes");
        assert!(text_to_canonical(key, &format!("[{SAVED_THEME_JSON}]")).is_ok());

        for bad_id in ["\"\"", "\" \"", "\" t1\"", "\"t1 \""] {
            let raw = format!("[{}]", SAVED_THEME_JSON.replace("\"t1\"", bad_id));
            assert!(
                text_to_canonical(key, &raw).is_err(),
                "ID {bad_id} 不应被接受"
            );
        }
        // 空名称与纯空白名称。
        for bad_name in ["\"\"", "\"  \""] {
            let raw = format!("[{}]", SAVED_THEME_JSON.replace("\"暖纸\"", bad_name));
            assert!(
                text_to_canonical(key, &raw).is_err(),
                "名称 {bad_name} 不应被接受"
            );
        }
        // 重名:同名、以及只差首尾空白的同名(去空白后判重);不同名的两条合法。
        let dup_exact =
            format!("[{SAVED_THEME_JSON},{SAVED_THEME_JSON}]").replace("\"t1\"", "\"t2\"");
        assert!(
            text_to_canonical(key, &dup_exact).is_err(),
            "同名条目应被拒绝"
        );
        let dup_trimmed = format!(
            "[{SAVED_THEME_JSON},{}]",
            SAVED_THEME_JSON
                .replace("\"暖纸\"", "\" 暖纸 \"")
                .replace("\"t1\"", "\"t2\"")
        );
        assert!(
            text_to_canonical(key, &dup_trimmed).is_err(),
            "去空白后同名应被拒绝"
        );
        let dup_id = format!(
            "[{SAVED_THEME_JSON},{}]",
            SAVED_THEME_JSON.replace("\"暖纸\"", "\"冷夜\"")
        );
        assert!(text_to_canonical(key, &dup_id).is_err(), "ID 重复应被拒绝");
        let two = format!(
            "[{SAVED_THEME_JSON},{}]",
            SAVED_THEME_JSON
                .replace("\"暖纸\"", "\"冷夜\"")
                .replace("\"t1\"", "\"t2\"")
        );
        assert!(
            text_to_canonical(key, &two).is_ok(),
            "不同 ID/名称的两条应被接受"
        );
    }

    /// 个人主题的原生 TOML 往返:渲染成表数组、读回规范文本一致;ID/名称未去重时**只丢弃违规条目**
    /// 并回报(加载是宽容路径),不因此让 `theme_saved_themes` 整键失效。
    #[test]
    fn saved_themes_roundtrip_through_native_toml() {
        let key = def("theme_saved_themes");
        let canonical = text_to_canonical(key, &format!("[{SAVED_THEME_JSON}]")).unwrap();
        let literal = canonical_to_toml_literal(key, &canonical).unwrap();
        assert!(
            !literal.starts_with('"') && literal.contains("\"id\" = ") && literal.contains("auto"),
            "应渲染为原生 TOML 表数组:{literal}"
        );
        let item = canonical_to_item(key, &canonical).unwrap();
        let back = item_to_canonical(key, &item).unwrap();
        assert_eq!(back.text, canonical);
        assert!(back.ignored.is_empty(), "{:?}", back.ignored);

        // 旧条目缺外观字段按枚举首项 standard 补齐，并保留原 ID。
        let old = parse_item(&literal.replace(", \"visual_style\" = \"forest\"", ""));
        let old_back = item_to_canonical(key, &old).unwrap();
        assert!(old_back.text.contains("\"id\":\"t1\""));
        assert!(old_back.text.contains("\"visual_style\":\"standard\""));
        assert!(!old_back.ignored.is_empty());
        assert!(text_to_canonical(key, &format!("[{}]", SAVED_THEME_JSON.replace("forest", "unknown"))).is_err());

        // 磁盘上两条同名:第一条生效,第二条被丢弃并回报一条说明。
        // 注意这里必须是 **TOML** 字面量(文件本身就是 TOML),不能拿上面的 JSON 常量硬拼。
        let inner = &literal[1..literal.len() - 1];
        let duplicated = parse_item(&format!("[{inner}, {inner}]"));
        let got = item_to_canonical(key, &duplicated).unwrap();
        assert_eq!(got.text, canonical, "只剩第一条");
        assert_eq!(got.ignored.len(), 1, "{:?}", got.ignored);
        assert!(got.ignored[0].contains("第 2 条"), "{:?}", got.ignored);
    }

    /// 新键的默认值与缺省口径:两个配色键取预设文件里的默认种子,个人主题默认空数组;窗口不透明度
    /// 默认 90,材质默认 none(方案 §3)。
    ///
    /// 两个种子的取值在这里按方案 §4.1 复核。它们直接来自 `presets/default-*.json`
    /// (`include_str!`,Rust 侧不另存一份),故这条断言的作用是「预设文件被改动时立刻发现」,
    /// 而不是维护第二份默认值。
    #[test]
    fn new_theme_and_window_defaults() {
        assert_eq!(def("theme_saved_themes").default, "[]");
        assert_eq!(def("window_opacity").default, "90");
        assert_eq!(def("window_material").default, "none");
        assert_eq!(def("theme_visual_style").default, "standard");
        assert!(matches!(
            def("theme_light_palette").kind,
            SettingKind::Struct(_)
        ));
        assert!(matches!(
            def("theme_saved_themes").kind,
            SettingKind::StructList(_)
        ));
        assert_eq!(
            def("theme_light_palette").default,
            "{\"background\":\"#ffffff\",\"foreground\":\"#202024\",\"accent\":\"#087f5b\",\"contrast\":45,\"gallery\":\"auto\"}"
        );
        assert_eq!(
            def("theme_dark_palette").default,
            "{\"background\":\"#181818\",\"foreground\":\"#f4f4f5\",\"accent\":\"#34d399\",\"contrast\":60,\"gallery\":\"auto\"}"
        );
        // 两个配色键的默认值就是预设文件的原文(`include_str!`,Rust 侧不另存),故此处只校验
        // 它的**形状**:字段集与 THEME_SEED 对齐,证明默认值确实来自那份文件而不是别处。
        // 具体色值不在 Rust 复核——那会变成第二份默认值;色值的规格断言在前端 presets.spec.ts。
        for key in ["theme_light_palette", "theme_dark_palette"] {
            let parsed: Json = serde_json::from_str(def(key).default).unwrap();
            let mut got: Vec<&str> = parsed
                .as_object()
                .unwrap_or_else(|| panic!("{key} 的默认值不是对象"))
                .keys()
                .map(String::as_str)
                .collect();
            got.sort_unstable();
            let mut want: Vec<&str> = THEME_SEED.fields.iter().map(|f| f.name).collect();
            want.sort_unstable();
            assert_eq!(got, want, "{key} 的默认值字段应与 THEME_SEED 对齐");
        }
        // 两个配色键都用种子结构定义,字段顺序是前端转换的接线依据。
        let names: Vec<&str> = THEME_SEED.fields.iter().map(|f| f.name).collect();
        assert_eq!(
            names,
            vec!["background", "foreground", "accent", "contrast", "gallery"]
        );
    }
}
