//! DB → config.toml 的一次性迁移(A1)。config.toml 已存在则 no-op(不覆盖用户已有配置,
//! 也不重复迁移);不存在则从 `app_config` 表把设置类键中「已偏离默认值」的部分迁到新
//! 文件,DB 里的旧行**不删**——保留是有意的回滚安全网:降级回旧版本仍能从 DB 正常读取。
//!
//! 迁移在同步上下文跑(调用方是普通函数,不 `async`);A2 接入启动流程时会包一层
//! `spawn_blocking`(SQLite 是同步阻塞 IO,硬约束:异步命令里的 rusqlite 调用不得占用
//! tokio 执行器线程)。

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension};

use super::file::{render_template, write_atomic, ConfigFileError};
use super::schema::{SettingKind, SETTING_DEFS};

/// 迁移入口。返回 `Ok(true)` 表示确实执行了迁移写盘,`Ok(false)` 表示 config.toml 已存在、
/// 本次是 no-op(供调用方决定要不要打一条"首次生成配置文件"级别的日志)。
pub fn migrate_if_needed(db_conn: &Connection, path: &Path) -> Result<bool, ConfigFileError> {
    if path.exists() {
        return Ok(false);
    }

    let mut overrides: BTreeMap<String, String> = BTreeMap::new();
    for def in SETTING_DEFS {
        let raw: Option<String> = db_conn
            .query_row(
                "SELECT value FROM app_config WHERE key = ?1",
                [def.key],
                |row| row.get(0),
            )
            .optional()?;
        let Some(raw) = raw else { continue };
        // 非法值(如 UInt/Float 键存了 "abc")→ 跳过该键、保持注释态回退默认(P2 #5):不得
        // 让 render_literal 的 parse().unwrap_or(0) 兜底把它静默写成 0,那会把"迁移失败"
        // 伪装成"用户把值设成了 0"这一完全不同的合法状态。
        let Some(normalized) = normalize_for_kind(def.kind, &raw) else {
            continue;
        };
        if normalized != def.default {
            overrides.insert(def.key.to_string(), normalized);
        }
    }

    let content = render_template(&overrides);
    write_atomic(path, &content)?;
    Ok(true)
}

/// 历史 DB 值 → schema 规范文本形式的归一化。`Bool`:历史代码里同一批布尔键混用过两种
/// 字符串编码——`face_enabled`/`exotic_enabled` 等写 `"1"`/`"0"`,`ai_hq_cache_enabled` 等写
/// `"true"`/`"false"`,而 `SettingKind::Bool` 的 TOML 字面量只认 `true`/`false`。在迁移这一步
/// 统一改写成规范形式,而非留到 `file.rs` 校验期才把合法旧值当成语法错误拒收;顺带避免
/// "新旧编码不同但语义相同"被误判成"用户改过"而多余地写入覆盖层。
///
/// `UInt`/`Float`:历史 DB 值理论上应已是合法数字,但不排除脏数据(如空字符串/非数字文本);
/// 本函数在此校验一次,非法则返回 `None`(P2 #5)——调用方据此跳过该键、保持注释态回退
/// 默认,而不是让 `file.rs::render_literal` 的 parse-or-zero 兜底把非法值静默写成 `0`。
/// 其余 kind 的历史存储形态与新格式逐字节一致,直接透传(无法校验、也无需校验)。
fn normalize_for_kind(kind: SettingKind, raw: &str) -> Option<String> {
    match kind {
        SettingKind::Bool => match raw {
            "1" | "true" | "TRUE" | "True" => Some("true".to_string()),
            "0" | "false" | "FALSE" | "False" => Some("false".to_string()),
            _ => None,
        },
        SettingKind::UInt => raw.parse::<u64>().ok().map(|n| n.to_string()),
        SettingKind::Float => raw.parse::<f64>().ok().map(|f| f.to_string()),
        SettingKind::Str | SettingKind::Path | SettingKind::Enum(_) => Some(raw.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_conn_with_app_config() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE app_config (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .unwrap();
        conn
    }

    /// 只有偏离默认值的键才会进入 overrides / 被渲染成实值行;等于默认值的键保持注释态。
    #[test]
    fn only_non_default_values_are_migrated() {
        let conn = make_conn_with_app_config();
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES ('thumb_size', '256')",
            [],
        )
        .unwrap(); // 偏离默认 512
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES ('log_level', 'info')",
            [],
        )
        .unwrap(); // 恰好等于默认值

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let migrated = migrate_if_needed(&conn, &path).unwrap();
        assert!(migrated);

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("\nthumb_size = 256\n"), "{content}");
        assert!(
            content.contains("# log_level = \"info\"\n"),
            "等于默认值的键应保持注释态:{content}"
        );
    }

    /// 历史 "1"/"0" 布尔编码被归一化为 "true"/"false" 再比较/写入。
    #[test]
    fn legacy_1_0_bool_is_normalized() {
        let conn = make_conn_with_app_config();
        // face_enabled 默认 "true";历史 DB 用 "1" 表达同一语义 → 归一后等于默认值,不应进 overrides。
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES ('face_enabled', '1')",
            [],
        )
        .unwrap();
        // exotic_auto_process 默认 "true";历史写 "0" 表示关闭 → 归一后 "false" ≠ 默认值,应进 overrides。
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES ('exotic_auto_process', '0')",
            [],
        )
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        migrate_if_needed(&conn, &path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();

        assert!(
            content.contains("# face_enabled = true\n"),
            "归一后等于默认值,应保持注释态:{content}"
        );
        assert!(
            content.contains("\nexotic_auto_process = false\n"),
            "归一后为 false、不等于默认 true,应写成实值行:{content}"
        );
    }

    /// config.toml 已存在 → no-op,不覆盖、不重复迁移。
    #[test]
    fn no_op_when_file_already_exists() {
        let conn = make_conn_with_app_config();
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES ('thumb_size', '999')",
            [],
        )
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "# 用户已有的配置\nthumb_size = 128\n").unwrap();

        let migrated = migrate_if_needed(&conn, &path).unwrap();
        assert!(!migrated);
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            content, "# 用户已有的配置\nthumb_size = 128\n",
            "既有文件不得被覆盖"
        );
    }

    /// 二次调用(文件已因第一次调用而存在)同样 no-op。
    #[test]
    fn second_call_is_no_op() {
        let conn = make_conn_with_app_config();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        assert!(migrate_if_needed(&conn, &path).unwrap());
        let first_content = std::fs::read_to_string(&path).unwrap();

        // 迁移后 DB 又变了(模拟旧版本继续运行一段时间又写了新值)也不该反映到文件——
        // 迁移只发生一次。
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES ('thumb_size', '64')",
            [],
        )
        .unwrap();
        assert!(!migrate_if_needed(&conn, &path).unwrap());
        let second_content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(first_content, second_content);
    }

    /// UInt 键存了非法值(非数字文本)→ 迁移跳过该键,保持注释态回退默认;不得被
    /// `render_literal` 的 parse-or-zero 兜底静默写成 `0`(P2 #5)。
    #[test]
    fn invalid_uint_value_is_skipped_not_coerced_to_zero() {
        let conn = make_conn_with_app_config();
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES ('thumb_size', 'abc')",
            [],
        )
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        migrate_if_needed(&conn, &path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();

        assert!(
            content.contains("# thumb_size = 512\n"),
            "非法迁移值应跳过、保持默认注释态,不应被静默写成 0:{content}"
        );
        assert!(
            !content.contains("\nthumb_size = 0\n"),
            "非法值不得被 render_literal 兜底写成 0:{content}"
        );
    }

    /// 未知布尔文本是脏数据，不得被“非 truthy 即 false”的兜底改写成用户主动关闭。
    #[test]
    fn invalid_bool_value_is_skipped_not_coerced_to_false() {
        let conn = make_conn_with_app_config();
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES ('face_enabled', 'enabled')",
            [],
        )
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        migrate_if_needed(&conn, &path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();

        assert!(
            content.contains("# face_enabled = true\n"),
            "非法布尔值应跳过并回退默认，不得被静默写成 false:{content}"
        );
        assert!(
            !content.contains("\nface_enabled = false\n"),
            "未知布尔文本不得被迁移成 false:{content}"
        );
    }

    /// 只有 QueryReturnedNoRows 才表示“该键未配置”；表缺失/损坏等 SQLite 错误必须阻止写盘，
    /// 否则首次失败仍会生成 config.toml，使后续启动永久跳过迁移。
    #[test]
    fn database_error_aborts_without_creating_config_file() {
        let conn = Connection::open_in_memory().unwrap(); // 故意不建 app_config 表
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let err = migrate_if_needed(&conn, &path).unwrap_err();
        assert!(matches!(err, ConfigFileError::Database(_)));
        let (message, line) = err.to_status_message();
        assert_eq!(message, "读取旧版配置数据库失败,本次未迁移配置文件");
        assert_eq!(line, None);
        assert!(!message.contains("app_config"), "IPC 状态不得透出底层 SQL");
        assert!(!path.exists(), "数据库错误时不得留下会封死重试的配置文件");
    }

    /// DB 旧行不删:迁移只读不写 app_config 表(回滚安全——降级到旧版本仍可读)。
    #[test]
    fn migration_does_not_delete_db_rows() {
        let conn = make_conn_with_app_config();
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES ('thumb_size', '256')",
            [],
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        migrate_if_needed(&conn, &path).unwrap();

        let still_there: String = conn
            .query_row(
                "SELECT value FROM app_config WHERE key = 'thumb_size'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(still_there, "256");
    }
}
