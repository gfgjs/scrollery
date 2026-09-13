pub mod boot;
pub mod connection;
pub mod migration;
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

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    /// 端到端回归:`NATURAL_CMP` collation 对含 ≥20 位连续数字的真实文件名做 `ORDER BY`
    /// **不再 panic**（替换前 lexicmp 在此 `n*10` 溢出→dev 刷屏），且给出正确自然序。
    /// 覆盖整条生产路径:register → SQLite 排序回调 → 自研 natural_cmp。
    #[test]
    fn natural_cmp_collation_sorts_long_digit_names_without_panic() {
        let conn = Connection::open_in_memory().unwrap();
        super::register_custom_collations(&conn).unwrap();
        conn.execute_batch(
            "CREATE TABLE t(name TEXT);
             INSERT INTO t(name) VALUES
               ('Camera_XHS_170200523981701027401z.jpg'),
               ('Camera_XHS_170200523737001027401z.jpg'),
               ('img2.jpg'),
               ('img10.jpg'),
               ('1488946725_201808222102505507635434873.jpg');",
        )
        .unwrap();
        let mut stmt = conn
            .prepare("SELECT name FROM t ORDER BY name COLLATE NATURAL_CMP ASC")
            .unwrap();
        let got: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        // 跨前缀首字符码点序:'1'(0x31) < 'C'(0x43) < 'i'(0x69);'C' 组内 21 位数字段
        // 首个差异位 7<9 → 737… 先于 981…;'i' 组内自然序 img2 < img10。全程无 panic。
        assert_eq!(
            got,
            vec![
                "1488946725_201808222102505507635434873.jpg",
                "Camera_XHS_170200523737001027401z.jpg",
                "Camera_XHS_170200523981701027401z.jpg",
                "img2.jpg",
                "img10.jpg",
            ]
        );
    }
}
