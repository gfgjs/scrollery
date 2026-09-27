pub mod boot;
pub mod connection;
pub mod models;
pub mod queries;
pub mod schema;

pub use connection::{create_read_pool, create_write_connection, DbPool, DbWriter};

use rusqlite::functions::FunctionFlags;
use rusqlite::Connection;

/// 注册全仓查询共用的自定义排序规则与标量函数。写连接与读池连接均经此单一入口注册
/// （见 `connection.rs`），保证任何连接上的查询都能解析 `NATURAL_CMP` / `TREE_SORT_KEY`。
///
/// - `NATURAL_CMP` collation：数字感知的自然序（文件名排序），委托溢出安全的
///   [`crate::utils::natural_sort::natural_cmp`]（替换原 `lexicmp::natural_cmp`——后者对 ≥20 位
///   连续数字的文件名 `n*10` 溢出致 dev 刷屏 panic + 排序塌方，见该模块文档）。
/// - `TREE_SORT_KEY(rel_path) -> BLOB` 标量函数：把 `/`-分隔 rel_path 编码为前序 DFS 排序键
///   （委托 [`crate::utils::path::encode_tree_sort_key`]），供 `push_order_by` 的 folder 分支
///   `ORDER BY TREE_SORT_KEY(d.rel_path)` 与内存 `build_dir_rank` **共用同一序逻辑**——两侧
///   同构（BLOB memcmp = Rust `Vec<u8>::cmp`）是内存/SQL 刚性等价契约成立的根据。标 `DETERMINISTIC`
///   （同输入恒同输出）利于查询优化器。
pub fn register_custom_collations(conn: &Connection) -> rusqlite::Result<()> {
    conn.create_collation("NATURAL_CMP", crate::utils::natural_sort::natural_cmp)?;
    conn.create_scalar_function(
        "TREE_SORT_KEY",
        1,
        FunctionFlags::SQLITE_DETERMINISTIC | FunctionFlags::SQLITE_UTF8,
        |ctx| {
            let rel_path: String = ctx.get(0)?;
            Ok(crate::utils::path::encode_tree_sort_key(&rel_path))
        },
    )?;
    Ok(())
}
