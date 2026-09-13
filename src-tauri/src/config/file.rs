//! config.toml 的读写:语法/类型校验、模板渲染、原子写盘、内容指纹。
//!
//! 用 `toml_edit` 而非 `serde` 反序列化整份配置到结构体的原因:后者一旦某个键类型不合法
//! 就整体反序列化失败,与本模块「单键类型错只警告、跳过该键、其余键照常生效」的容错契约
//! 冲突;`toml_edit` 保留原始文档树(含注释/排版),让 `set_value`/`unset_value` 能只改动
//! 目标键的值节点而不扰动其余内容,这也是 serde 路线做不到的。

use std::collections::BTreeMap;
use std::path::Path;

use thiserror::Error;
use toml_edit::{DocumentMut, Entry, Item, Value};

use super::schema::{section_title, SettingDef, SettingKind, SETTING_DEFS};

/// config.toml 读写过程中的错误。**不含任何原始异常字符串直通 IPC 的设计**(硬约束:IPC
/// 错误不得泄漏内部字符串)——本类型仍是内部错误,A2 接 IPC 时再映射为稳定 code/variant。
#[derive(Debug, Error)]
pub enum ConfigFileError {
    /// 整文件级 TOML 语法错误(结构性,如括号不匹配、非法转义)。行列号来自 toml_edit 的 span。
    #[error("config.toml 第 {line} 行第 {column} 列存在语法错误:{message}")]
    Syntax {
        line: usize,
        column: usize,
        message: String,
    },
    #[error("读写 config.toml 失败:{0}")]
    Io(#[from] std::io::Error),
    #[error("读取旧版配置数据库失败:{0}")]
    Database(#[from] rusqlite::Error),
}

impl ConfigFileError {
    /// A2:转成面向 IPC `get_config_status`/`config-file-error` 的精简描述——
    /// `(message, line)`,`line` 仅语法错误时有值。**不透传 `Io` 变体的原始 `Display`**:
    /// 该分支可能内嵌完整文件路径(含用户名等),违反「IPC 错误不得泄漏内部原始字符串」硬约束;
    /// `Syntax` 分支的 message 本就是 toml_edit 给出的、指着用户自己文件内容说话的面向人类文案,
    /// 展示它正是本功能的意义所在,不算泄漏。
    pub fn to_status_message(&self) -> (String, Option<usize>) {
        match self {
            ConfigFileError::Syntax { line, .. } => (self.to_string(), Some(*line)),
            ConfigFileError::Io(_) => (
                "读写配置文件失败,请检查文件是否被其他程序占用或权限不足".to_string(),
                None,
            ),
            ConfigFileError::Database(_) => (
                "读取旧版配置数据库失败,本次未迁移配置文件".to_string(),
                None,
            ),
        }
    }
}

/// 单键级、非致命的校验问题(整文件仍加载成功,该键回退默认值)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyWarning {
    /// 出问题的键名(未知键场景下就是文件里出现的原始键名,可能不在 schema 内)。
    pub key: String,
    /// 面向用户的中文说明(A2 转发到设置页时直接展示,故本身已是可读文案,不再需要二次翻译)。
    pub message: String,
}

/// 一次成功加载的结果:整文件语法合法,但个别键可能因类型不符/未知而被跳过(记入 `warnings`)。
#[derive(Debug)]
pub struct LoadedConfig {
    /// 生效值表:仅含**文件中确实出现且校验通过**的键(注释态/文件缺省的键不在此表,
    /// 由消费方回退 `SettingDef.default` —— `ConfigManager::get` 已内置该回退)。
    pub values: BTreeMap<String, String>,
    pub warnings: Vec<KeyWarning>,
    /// 原始文档树,供 `set_value`/`unset_value` 就地改写复用(避免调用方重新解析一遍文件)。
    pub doc: DocumentMut,
}

/// FNV-1a 64 位哈希:用于「最近自写」指纹比对(watcher 防自身写回环用),不追求密码学强度,
/// 只需对内容变化敏感、纯函数、零新增依赖(标准库/已有依赖均无现成 64 位非加密哈希导出)。
pub fn fingerprint(content: &str) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0100_0000_01b3;
    let mut hash = FNV_OFFSET;
    for &byte in content.as_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// 字节偏移 → 1 基的(行号, 列号),用于把 toml_edit 的 span 转成人类可读的报错位置。
/// O(n) 扫描:配置文件体量小(数十键),不值得为此引入行首偏移索引之类的复杂度。
fn offset_to_line_col(text: &str, offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut col = 1usize;
    for (i, ch) in text.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// 把文本解析为可编辑文档;语法错误时带行列号(不下沉具体异常字符串给 IPC 消费方,
/// message 已是 toml_edit 给出的面向人类文案)。
pub fn parse_doc(text: &str) -> Result<DocumentMut, ConfigFileError> {
    text.parse::<DocumentMut>().map_err(|e| {
        let (line, column) = e
            .span()
            .map(|s| offset_to_line_col(text, s.start))
            .unwrap_or((0, 0));
        ConfigFileError::Syntax {
            line,
            column,
            message: e.message().to_string(),
        }
    })
}

/// 按 `SettingKind` 校验单个 TOML 值节点,合法则返回其规范文本形式(与 `SettingDef.default`
/// 同一形态,便于上层直接字符串比较/存表),非法返回中文错误说明(调用方转成 `KeyWarning`)。
fn validate_value(kind: SettingKind, item: &Item) -> Result<String, String> {
    match kind {
        SettingKind::Bool => item
            .as_bool()
            .map(|b| b.to_string())
            .ok_or_else(|| "应为布尔值 true 或 false".to_string()),
        SettingKind::UInt => item
            .as_integer()
            .filter(|n| *n >= 0)
            .map(|n| n.to_string())
            .ok_or_else(|| "应为非负整数".to_string()),
        SettingKind::Float => item
            .as_float()
            .or_else(|| item.as_integer().map(|n| n as f64))
            .map(|f| f.to_string())
            .ok_or_else(|| "应为数字".to_string()),
        SettingKind::Str | SettingKind::Path => item
            .as_str()
            .map(std::string::ToString::to_string)
            .ok_or_else(|| "应为字符串(用双引号包裹)".to_string()),
        SettingKind::Enum(options) => {
            let s = item
                .as_str()
                .ok_or_else(|| "应为字符串(用双引号包裹)".to_string())?;
            if options.contains(&s) {
                Ok(s.to_string())
            } else {
                Err(format!(
                    "取值 \"{s}\" 不在允许范围内,允许值:{}",
                    options.join(" / ")
                ))
            }
        }
    }
}

/// 加载 config.toml:文件不存在按空文档处理(全部键回退默认,不算警告——正常初始态);
/// 整文件语法错 → `Err`(调用方按契约保留上一次成功配置,不得让半解析结果流入生效值表);
/// 单键类型不符 / 文件中出现 schema 之外的未知键 → 记入 `warnings`,该键跳过、其余键照常生效。
pub fn load(path: &Path) -> Result<LoadedConfig, ConfigFileError> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(ConfigFileError::Io(e)),
    };
    let doc = parse_doc(&text)?;

    let mut values = BTreeMap::new();
    let mut warnings = Vec::new();

    for def in SETTING_DEFS {
        // 键在 schema 内、但文件里没写(即注释态或整个缺失)→ 不进 values、不算警告,由
        // `ConfigManager::get` 回退 `def.default`,与「注释态本来就不在 doc 值里」的模板设计一致。
        if let Some(item) = doc.get(def.key) {
            match validate_value(def.kind, item) {
                Ok(s) => {
                    values.insert(def.key.to_string(), s);
                }
                Err(message) => warnings.push(KeyWarning {
                    key: def.key.to_string(),
                    message,
                }),
            }
        }
    }

    // 未知键扫描:文件顶层出现但不在当前 schema 内的键(旧版本已删的键 / 用户手误敲错的键 /
    // 从更新版本降级回来的键)。只警告不报错——不能因为一个陌生键就让整份配置失效。
    let known: std::collections::HashSet<&str> = SETTING_DEFS.iter().map(|d| d.key).collect();
    for (key, _item) in doc.iter() {
        if !known.contains(key) {
            warnings.push(KeyWarning {
                key: key.to_string(),
                message: "未知配置键(不在当前版本可识别的设置项内),已忽略".to_string(),
            });
        }
    }

    Ok(LoadedConfig {
        values,
        warnings,
        doc,
    })
}

/// 把规范文本值渲染成对应 kind 的 TOML 字面量源码(`Value` 的 `Display` 输出,字符串类会
/// 自动加引号并按 TOML 规则转义;裸词类原样输出)。解析失败时回退各 kind 的零值/false——
/// 只会发生在 `SETTING_DEFS.default` 或已通过 `validate_value` 校验过的值上,理论不可达,
/// 兜底纯粹是不 `unwrap` panic。
fn render_literal(kind: SettingKind, raw: &str) -> String {
    let value: Value = match kind {
        SettingKind::Bool => raw.parse::<bool>().unwrap_or(false).into(),
        SettingKind::UInt => raw.parse::<i64>().unwrap_or(0).into(),
        SettingKind::Float => raw.parse::<f64>().unwrap_or(0.0).into(),
        SettingKind::Str | SettingKind::Path | SettingKind::Enum(_) => raw.into(),
    };
    value.to_string()
}

/// 单键的模板片段:注释说明行 + (`# key = 默认值` 或 `key = 覆盖值`)+ 尾随空行。
fn render_key_block(def: &SettingDef, override_value: Option<&str>) -> String {
    let mut out = String::new();
    for line in def.comment_zh.lines() {
        out.push_str("# ");
        out.push_str(line);
        out.push('\n');
    }
    match override_value {
        Some(v) => {
            out.push_str(&format!("{} = {}\n", def.key, render_literal(def.kind, v)));
        }
        None => {
            out.push_str(&format!(
                "# {} = {}\n",
                def.key,
                render_literal(def.kind, def.default)
            ));
        }
    }
    out.push('\n');
    out
}

const HEADER: &str = "\
# ══════════════════════════════════════════════════════════════════════════
# Scrollery 应用配置文件(config.toml)
# ══════════════════════════════════════════════════════════════════════════
#
# 本文件是什么:Scrollery 用户可配置设置(界面/画廊/缩略图/AI 等)的唯一存放处。每个
#   设置项下方先有一行中文说明,再有一行注释掉的「# 键名 = 默认值」。
#
# 注释态 = 使用默认值:凡是以 `#` 开头的「# 键名 = ...」行,表示该项当前生效值就是
#   显示的默认值,你尚未覆盖它。
#
# 取消注释即覆盖:删掉行首的「# 」并按需修改右侧的值,保存文件即视为你显式设置了该项
#   (此后不再随程序升级自动变化,除非你再次注释掉该行或删除整行)。
#
# 保存即时生效:大多数设置项在保存本文件后立即生效,无需重启应用;少数项需要重启才能
#   生效,已在对应说明行中注明。
#
# 语法错误时保持旧配置并在设置页提示:若本文件出现 TOML 语法错误,应用会继续使用上一次
#   成功加载的配置,并在设置页给出提示——不会因为文件写错而崩溃或丢失其余设置。
#
";

/// 渲染完整模板:`overrides` 里有的键写成实值行,其余键写成注释态默认值行。用于:
/// (a) 首次生成全新 config.toml(无覆盖 = 全默认注释态);
/// (b) 迁移旧 DB 值(overrides = 与默认不同的历史值);
/// (c) `set_value`/`unset_value` 之外偶尔需要整份重渲染的场景(当前批次未使用,留作 A2 备用)。
pub fn render_template(overrides: &BTreeMap<String, String>) -> String {
    let mut out = String::from(HEADER);
    let mut last_section = "";
    for def in SETTING_DEFS {
        if def.section != last_section {
            out.push_str(&format!(
                "# ── {} ──────────────────────────────────────────────\n\n",
                section_title(def.section)
            ));
            last_section = def.section;
        }
        out.push_str(&render_key_block(
            def,
            overrides.get(def.key).map(String::as_str),
        ));
    }
    out
}

/// 判断某键是否**连注释态都不在**给定原始文本里(逐行匹配行首「可选空白 + 可选一串 `#` +
/// 空白 + 键名 + 空白 + `=`」)。用于版本升级后自愈补全新增键(见 `mod.rs::load_or_init`)。
///
/// 用**原始文本子串匹配**而非解析后的 `DocumentMut` 树:被注释掉的行根本不会出现在
/// doc 的值树里,若改用 doc 判断会把"用户特意保持默认(注释态)"的键也误判成缺失,
/// 每次启动都重复追加。
fn content_mentions_key(content: &str, key: &str) -> bool {
    content.lines().any(|line| {
        let trimmed = line.trim_start().trim_start_matches('#').trim_start();
        trimmed
            .strip_prefix(key)
            .map(|rest| rest.trim_start().starts_with('='))
            .unwrap_or(false)
    })
}

/// 找出 schema 中新增、而旧配置文件里连注释态都没有的键(典型场景:应用升级后 schema
/// 新增了设置项,但用户的 config.toml 是旧版本写出的),把这些键的模板片段追加到文件末尾。
/// 无缺失键时返回 `None`(调用方据此判断是否需要重新写盘,避免每次启动都触碰文件 mtime)。
///
/// 只追加、不改动既有内容/注释/排版——追加位置在文件末尾,按缺失键所属 section 分组,
/// 用专门的横幅注释与原有分段区分开,避免与用户可能已手改过的原分段混淆。
pub fn append_missing_keys(existing_content: &str) -> Option<String> {
    let missing: Vec<&SettingDef> = SETTING_DEFS
        .iter()
        .filter(|d| !content_mentions_key(existing_content, d.key))
        .collect();
    if missing.is_empty() {
        return None;
    }

    let mut out = existing_content.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
    out.push_str(
        "# ── 新增设置(应用版本升级自动补全,内容与上文其余分段无关)──────────────────\n\n",
    );
    let mut last_section = "";
    for def in &missing {
        if def.section != last_section {
            out.push_str(&format!("# ─ {} ─\n", section_title(def.section)));
            last_section = def.section;
        }
        out.push_str(&render_key_block(def, None));
    }
    Some(out)
}

/// 就地修改文档中某键的值(新增或覆盖)。**不经 `Table::insert(&str, Item)`**——该方法会对
/// 已存在的键调用 `Key::fmt()` 清空其前缀装饰(即行首注释),与"保留既有注释与排版"的硬
/// 约束冲突;改经 `Entry` API 的 `OccupiedEntry::insert`,只替换值节点、不触碰 key 的装饰。
///
/// `raw_value` 是规范文本形式(与 `SettingDef.default`/`LoadedConfig.values` 同源);未知键
/// (不在 schema 内)按 `Str` 处理写成带引号字符串——`ConfigManager::set_and_persist` 只应对
/// 已知键调用本函数,未知键分支纯粹是防御性兜底,不是本函数鼓励的用法。
pub fn set_value(doc: &mut DocumentMut, key: &str, raw_value: &str) {
    let kind = SettingDef::find(key).map_or(SettingKind::Str, |d| d.kind);
    let value: Value = match kind {
        SettingKind::Bool => raw_value.parse::<bool>().unwrap_or(false).into(),
        SettingKind::UInt => raw_value.parse::<i64>().unwrap_or(0).into(),
        SettingKind::Float => raw_value.parse::<f64>().unwrap_or(0.0).into(),
        SettingKind::Str | SettingKind::Path | SettingKind::Enum(_) => raw_value.into(),
    };
    match doc.entry(key) {
        Entry::Occupied(mut occ) => {
            occ.insert(Item::Value(value));
        }
        Entry::Vacant(vac) => {
            vac.insert(Item::Value(value));
        }
    }
}

/// 删除文档中某键的实值行(用户「回默认」——文件里从此不再出现该键,`load()` 会用
/// `SettingDef.default` 回退,视觉上等价于恢复成注释态,只是不会自动补回那行注释文本;
/// 若想连同模板注释一并恢复,调用方可改走整份 `render_template` 重渲染)。
pub fn unset_value(doc: &mut DocumentMut, key: &str) {
    doc.remove(key);
}

/// 原子写盘:同目录(=同卷)`*.tmp` 写入 + rename 覆盖目标文件(项目硬约束:派生文件不得
/// 直接覆盖写,防止写到一半崩溃/断电留下半份文件)。父目录不存在则先建。成功后返回写入
/// 内容的指纹(FNV-1a),供调用方记为「最近自写」供 watcher 环节对照跳过自触发。
pub fn write_atomic(path: &Path, content: &str) -> Result<u64, ConfigFileError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension("toml.tmp");
    std::fs::write(&tmp_path, content)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(fingerprint(content))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// 模板含全部键,且默认状态下每一行都是注释态(不存在裸的 `key = value` 实值行)。
    #[test]
    fn template_contains_all_keys_all_commented_when_no_overrides() {
        let rendered = render_template(&BTreeMap::new());
        for def in SETTING_DEFS {
            let commented = format!("# {} = ", def.key);
            assert!(
                rendered.contains(&commented),
                "模板缺少 {} 的注释态默认值行",
                def.key
            );
            let live = format!("\n{} = ", def.key);
            assert!(
                !rendered.contains(&live),
                "无覆盖时 {} 不应以实值行出现",
                def.key
            );
        }
    }

    /// overrides 里的键渲染为实值行(无 `#` 前缀),其余键仍是注释态。
    #[test]
    fn template_renders_overrides_as_live_lines() {
        let mut overrides = BTreeMap::new();
        overrides.insert("thumb_size".to_string(), "256".to_string());
        overrides.insert("ai_hq_cache_enabled".to_string(), "true".to_string());
        let rendered = render_template(&overrides);
        assert!(rendered.contains("\nthumb_size = 256\n"));
        assert!(rendered.contains("\nai_hq_cache_enabled = true\n"));
        // 未覆盖的键仍是注释态。
        assert!(rendered.contains("# log_level = \"info\"\n"));
    }

    /// 整文件语法错误 → Err,且带出行号列号(具体数值不做强断言,toml_edit 内部实现可能
    /// 微调 span 边界;只要求命中错误分支且行列号非零)。
    #[test]
    fn load_reports_line_col_on_syntax_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "thumb_size = [1, 2\n").unwrap(); // 未闭合数组
        let err = load(&path).unwrap_err();
        match err {
            ConfigFileError::Syntax { line, column, .. } => {
                assert!(line >= 1 && column >= 1, "行列号应为 1 基有效值");
            }
            other => panic!("应为语法错误,实际:{other:?}"),
        }
    }

    /// 单键类型错(布尔键写了字符串)→ 记警告并跳过该键,其余合法键照常进入 values。
    #[test]
    fn single_bad_key_warns_and_others_still_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "ai_hq_cache_enabled = \"yes\"\nthumb_size = 256\n").unwrap();
        let loaded = load(&path).unwrap();
        assert!(loaded
            .warnings
            .iter()
            .any(|w| w.key == "ai_hq_cache_enabled"));
        assert_eq!(
            loaded.values.get("thumb_size").map(String::as_str),
            Some("256")
        );
        assert!(!loaded.values.contains_key("ai_hq_cache_enabled"));
    }

    /// schema 之外的未知键 → 警告,不影响文件其余部分加载。
    #[test]
    fn unknown_key_warns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "totally_made_up_key = 1\nthumb_size = 256\n").unwrap();
        let loaded = load(&path).unwrap();
        assert!(loaded
            .warnings
            .iter()
            .any(|w| w.key == "totally_made_up_key"));
        assert_eq!(
            loaded.values.get("thumb_size").map(String::as_str),
            Some("256")
        );
    }

    /// set_value 就地改写已存在的键时,保留该键原有的前缀注释行(不被清空)。
    #[test]
    fn set_value_preserves_existing_comment() {
        let text = "# 我的自定义说明\nthumb_size = 512\n";
        let mut doc = parse_doc(text).unwrap();
        set_value(&mut doc, "thumb_size", "256");
        let out = doc.to_string();
        assert!(out.contains("# 我的自定义说明"), "写回后注释应仍在:{out}");
        assert!(out.contains("thumb_size = 256"));
    }

    /// unset_value 删除实值行,使该键从文档中消失(读取侧回退默认值)。
    #[test]
    fn unset_value_removes_live_line() {
        let text = "thumb_size = 256\nlog_level = \"debug\"\n";
        let mut doc = parse_doc(text).unwrap();
        unset_value(&mut doc, "thumb_size");
        let out = doc.to_string();
        assert!(!out.contains("thumb_size"));
        assert!(out.contains("log_level"));
    }

    /// write_atomic 产物存在、内容正确,且不残留同名 .tmp 文件。
    #[test]
    fn write_atomic_leaves_no_tmp_residue() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let fp = write_atomic(&path, "thumb_size = 256\n").unwrap();
        assert!(path.exists());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "thumb_size = 256\n"
        );
        assert_eq!(fp, fingerprint("thumb_size = 256\n"));
        let tmp = path.with_extension("toml.tmp");
        assert!(!tmp.exists(), "写盘后不应残留 .tmp 文件");
    }

    /// append_missing_keys:构造一份"缺了某个 schema 键(连注释都没有)"的旧文件,应被追加;
    /// 已包含全部键(即使是注释态)的文件应返回 None(不重复追加)。
    #[test]
    fn append_missing_keys_adds_absent_and_skips_when_complete() {
        // 全量模板一定包含所有键 → 视为"完整",不应追加。
        let full = render_template(&BTreeMap::new());
        assert!(append_missing_keys(&full).is_none());

        // 去掉某一行(模拟旧版本 schema 还没有这个键)。
        let without_one: String = full
            .lines()
            .filter(|l| !l.contains("thumb_webp_quality"))
            .map(|l| format!("{l}\n"))
            .collect();
        let healed = append_missing_keys(&without_one).expect("应检测到缺失键并追加");
        assert!(healed.contains("thumb_webp_quality"));
        // 追加是纯粹的"补",原有内容原样保留(仍能找到未被动过的其他键)。
        assert!(healed.contains("thumb_size"));
    }
}
