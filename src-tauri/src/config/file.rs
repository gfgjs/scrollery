//! config.toml 的读写:语法/类型校验、模板渲染、原子写盘、内容指纹。
//!
//! 用 `toml_edit` 而非 `serde` 反序列化整份配置到结构体的原因:后者一旦某个键类型不合法
//! 就整体反序列化失败,与本模块「单键类型错只警告、跳过该键、其余键照常生效」的容错契约
//! 冲突;`toml_edit` 保留原始文档树(含注释/排版),让 `set_value`/`unset_value` 能只改动
//! 目标键的值节点而不扰动其余内容,这也是 serde 路线做不到的。

use std::collections::BTreeMap;
use std::path::Path;

use thiserror::Error;
use toml_edit::{DocumentMut, Entry};

use super::schema::{section_title, SettingDef, SETTING_DEFS};
use super::value;

/// config.toml 读写过程中的内部错误；传到 IPC 前须映射为稳定码，避免泄漏原始异常。
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
    /// 调用方给的键不在 schema 内(内部编程错误或跨模块误用,不是用户文件的问题)。
    #[error("未知配置键:{0}")]
    UnknownKey(String),
    /// 值不符合键的类型约束(在进入写盘前的校验层被拦下)。
    #[error("设置值不合法:{0}")]
    InvalidValue(String),
}

impl ConfigFileError {
    /// 转成面向 IPC `get_config_status`/`config-file-error` 的精简描述——
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
            // 两个新变体的 message 都是本模块自己写的中文说明,不含路径与内部异常串,可直接展示。
            ConfigFileError::UnknownKey(_) | ConfigFileError::InvalidValue(_) => {
                (self.to_string(), None)
            }
        }
    }
}

/// 单键级、非致命的校验问题(整文件仍加载成功,该键回退默认值)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyWarning {
    /// 出问题的键名(未知键场景下就是文件里出现的原始键名,可能不在 schema 内)。
    pub key: String,
    /// 面向用户的中文说明，由设置页直接展示。
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

/// 加载 config.toml:文件不存在按空文档处理(全部键回退默认,不算警告——正常初始态);
/// 整文件语法错 → `Err`(调用方按契约保留上一次成功配置,不得让半解析结果流入生效值表);
/// 单键类型不符 / 文件中出现 schema 之外的未知键 → 记入 `warnings`,该键跳过、其余键照常生效。
///
/// 结构类键内部的未知 ID / 表外窗口 label / 多余字段**只忽略该条目**并记一条警告,整键仍生效
/// (方案 §5.4:一个陌生条目不该让无关设置失效)。值的形态转换与校验全部交 `value` 模块。
pub fn load(path: &Path) -> Result<LoadedConfig, ConfigFileError> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(ConfigFileError::Io(e)),
    };
    let doc = parse_doc(&text)?;
    Ok(validate_doc(doc))
}

/// 校验一份**已解析**的文档,产出生效值表与警告。供三条路径共用:启动与外部重读的 load,以及提交
/// 写盘后「从最终文档重建生效值」(见 ConfigManager 的 write_patch_locked)——后者使内存快照与磁盘
/// 内容永不背离(磁盘上由其他写入方或外部编辑留下的键也会一并进入快照,而不是等下一次重读才补上)。
///
/// 返回值恒为 Ok:单键级问题只记警告并跳过该键(该键回退 schema 默认值),整文件语法错在 parse_doc
/// 阶段就已拦下。
pub fn validate_doc(doc: DocumentMut) -> LoadedConfig {
    let mut values = BTreeMap::new();
    let mut warnings = Vec::new();

    for def in SETTING_DEFS {
        // 键在 schema 内、但文件里没写(即注释态或整个缺失)→ 不进 values、不算警告,由
        // `ConfigManager::get` 回退 `def.default`,与「注释态本来就不在 doc 值里」的模板设计一致。
        if let Some(item) = doc.get(def.key) {
            match value::item_to_canonical(def, item) {
                Ok(parsed) => {
                    for note in &parsed.ignored {
                        warnings.push(KeyWarning {
                            key: def.key.to_string(),
                            message: note.clone(),
                        });
                    }
                    values.insert(def.key.to_string(), parsed.text);
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

    LoadedConfig {
        values,
        warnings,
        doc,
    }
}

/// 把规范文本值渲染成对应 kind 的 TOML 字面量源码。结构类键渲染成**原生 TOML 数组/内联表**
/// (方案 §4:文件里不得嵌套 JSON 字符串)。默认值或覆盖值渲染失败时中止模板生成,
/// 避免用占位内容覆盖有效配置。
fn render_literal(def: &SettingDef, raw: &str) -> Result<String, ConfigFileError> {
    value::canonical_to_toml_literal(def, raw).map_err(ConfigFileError::InvalidValue)
}

/// 单键的模板片段:注释说明行 + (`# key = 默认值` 或 `key = 覆盖值`)+ 尾随空行。
fn render_key_block(
    def: &SettingDef,
    override_value: Option<&str>,
) -> Result<String, ConfigFileError> {
    let mut out = String::new();
    for line in def.comment_zh.lines() {
        out.push_str("# ");
        out.push_str(line);
        out.push('\n');
    }
    match override_value {
        Some(v) => {
            out.push_str(&format!("{} = {}\n", def.key, render_literal(def, v)?));
        }
        None => {
            out.push_str(&format!(
                "# {} = {}\n",
                def.key,
                render_literal(def, def.default)?
            ));
        }
    }
    out.push('\n');
    Ok(out)
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
/// (b) 显式恢复默认配置,或运行期配置文件被外部删除后的编辑。
pub fn render_template(overrides: &BTreeMap<String, String>) -> Result<String, ConfigFileError> {
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
        )?);
    }
    Ok(out)
}

/// 就地修改文档中某键的值(新增或覆盖)。**不经 `Table::insert(&str, Item)`**——该方法会对
/// 已存在的键调用 `Key::fmt()` 清空其前缀装饰(即行首注释),与"保留既有注释与排版"的硬
/// 约束冲突;改经 `Entry` API 的 `OccupiedEntry::insert`,只替换值节点、不触碰 key 的装饰。
///
/// `raw_value` 是规范文本形式(与 `SettingDef.default`/`LoadedConfig.values` 同源),
/// 调用方先定位设置定义;转换失败沿提交链返回,整批不会保存部分键。
pub fn set_value(
    doc: &mut DocumentMut,
    def: &SettingDef,
    raw_value: &str,
) -> Result<(), ConfigFileError> {
    let new_item =
        value::canonical_to_item(def, raw_value).map_err(ConfigFileError::InvalidValue)?;
    match doc.entry(def.key) {
        Entry::Occupied(mut occ) => {
            occ.insert(new_item);
        }
        Entry::Vacant(vac) => {
            vac.insert(new_item);
        }
    }
    Ok(())
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
