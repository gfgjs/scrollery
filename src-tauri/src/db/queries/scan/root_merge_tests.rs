use super::*;
use crate::db::queries::{self as q, upsert_directory};

struct Fixture {
    conn: Connection,
    temp: tempfile::TempDir,
    root: i64,
    directory: i64,
    item: i64,
    parent_path: String,
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().join("library");
    let child = parent.join("collection").join("books");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::write(child.join("note.md"), "原始正文").unwrap();
    std::fs::write(temp.path().join("revision.txt"), "我的修订稿").unwrap();
    let conn = Connection::open(temp.path().join("fixture.sqlite")).unwrap();
    crate::db::connection::apply_pragmas(&conn).unwrap();
    crate::db::schema::initialize_schema(&conn).unwrap();
    let root = insert_scan_root(&conn, &child.to_string_lossy().replace('\\', "/"), None).unwrap();
    let directory = upsert_directory(&conn, root, None, "", "books", 0, None).unwrap();
    conn.execute(
        "INSERT INTO media_items (directory_id,file_name,file_size,file_mtime,file_format,
         media_type,sort_datetime,cache_key,is_favorited,rating) VALUES (?1,'note.md',12,100,'md','document',100,123,1,5)",
        [directory],
    ).unwrap();
    let item = conn.last_insert_rowid();
    conn.execute("INSERT INTO tags (name) VALUES (?1)", ["重要"])
        .unwrap();
    let tag = conn.last_insert_rowid();
    conn.execute("INSERT INTO item_tags VALUES (?1,?2)", params![item, tag])
        .unwrap();
    conn.execute("INSERT INTO albums (name) VALUES (?1)", ["书架"])
        .unwrap();
    let album = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO album_items (album_id,item_id) VALUES (?1,?2)",
        params![album, item],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO document_versions (item_id,label,storage,abs_path,source,is_current)
         VALUES (?1,'修订','appdata',?2,'user',1)",
        params![item, temp.path().join("revision.txt").to_string_lossy()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO reader_bookmarks (item_id,locator,label) VALUES (?1,'cfi:probe','书签')",
        [item],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO reading_progress (item_id,position) VALUES (?1,'cfi:probe')",
        [item],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO reader_book_prefs (item_id,prefs) VALUES (?1,?2)",
        params![item, r#"{"encoding":"utf-8"}"#],
    )
    .unwrap();
    Fixture {
        conn,
        temp,
        root,
        directory,
        item,
        parent_path: parent.to_string_lossy().replace('\\', "/"),
    }
}

fn merge(fixture: &mut Fixture) -> Result<ScanRoot> {
    merge_scan_root_children(
        &mut fixture.conn,
        &fixture.parent_path,
        None,
        &volume(),
        &[source(fixture.root, "collection/books")],
    )
}

fn source(root_id: i64, relative_path: &str) -> ScanRootMergeSource {
    ScanRootMergeSource {
        root_id,
        relative_path: relative_path.into(),
    }
}

fn volume() -> NewVolume {
    NewVolume {
        stable_id: "root-merge-fixture".into(),
        label: None,
        kind: crate::db::models::VolumeKind::Local,
        last_mount_path: None,
        last_seen: None,
        is_online: true,
    }
}

fn snapshot(conn: &Connection) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
    [
        "SELECT * FROM scan_roots ORDER BY id",
        "SELECT * FROM directories ORDER BY id",
        "SELECT * FROM media_items ORDER BY id",
        "SELECT * FROM document_versions ORDER BY id",
        "SELECT * FROM item_tags ORDER BY item_id,tag_id",
        "SELECT * FROM album_items ORDER BY album_id,item_id",
        "SELECT * FROM reader_bookmarks ORDER BY id",
        "SELECT * FROM reading_progress ORDER BY item_id",
        "SELECT * FROM reader_book_prefs ORDER BY item_id",
        "SELECT * FROM volumes ORDER BY id",
        "SELECT * FROM directory_move_journal ORDER BY id",
        "SELECT * FROM sqlite_sequence ORDER BY name",
    ]
    .iter()
    .map(|sql| {
        let mut statement = conn.prepare(sql).unwrap();
        let count = statement.column_count();
        let rows = statement
            .query_map([], |row| (0..count).map(|column| row.get(column)).collect())
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        rows
    })
    .collect()
}

#[test]
fn merge_scan_roots_preserves_asset_identity_and_user_state() {
    let mut fixture = fixture();
    set_scan_root_hidden(&fixture.conn, fixture.root, true).unwrap();
    let audio_path = std::path::Path::new(&fixture.parent_path).join("collection/audio");
    std::fs::create_dir_all(&audio_path).unwrap();
    let audio_root = insert_scan_root(&fixture.conn, &audio_path.to_string_lossy(), None).unwrap();
    set_scan_root_hidden(&fixture.conn, audio_root, true).unwrap();
    let audio_dir =
        upsert_directory(&fixture.conn, audio_root, None, "", "audio", 0, None).unwrap();
    let nested = upsert_directory(
        &fixture.conn,
        fixture.root,
        Some(fixture.directory),
        "notes",
        "notes",
        1,
        None,
    )
    .unwrap();
    let parent = merge_scan_root_children(
        &mut fixture.conn,
        &fixture.parent_path,
        None,
        &volume(),
        &[
            source(fixture.root, "collection/books"),
            source(audio_root, "collection/audio"),
        ],
    )
    .unwrap();
    assert!(parent.is_hidden, "合并隐藏库不能意外公开其中的资产");
    let item = q::get_media_detail(&fixture.conn, fixture.item)
        .expect("合并根目录必须保留媒体ID，不能级联删除后重扫");
    assert_eq!(item.item.id, fixture.item);
    assert!(item.item.is_favorited);
    assert_eq!(item.item.rating, 5);
    assert_eq!(item.item.cache_key, 123);
    let directory: (i64, String, i64, Vec<u8>) = fixture
        .conn
        .query_row(
            "SELECT root_id,rel_path,depth,tree_sort_key FROM directories WHERE id=?1",
            [fixture.directory],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(directory.0, parent.id);
    assert_eq!(directory.1, "collection/books");
    assert_eq!(directory.2, 2);
    assert_eq!(
        directory.3,
        crate::utils::path::encode_tree_sort_key("collection/books")
    );
    let hierarchy: (String, i64, bool) = fixture.conn.query_row(
        "SELECT d.rel_path,d.parent_id,
         (SELECT parent_id FROM directories WHERE id=?2)=(SELECT parent_id FROM directories WHERE id=?3)
         FROM directories d WHERE d.id=?1",
        params![nested, audio_dir, fixture.directory], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
    ).unwrap();
    assert_eq!(
        hierarchy,
        ("collection/books/notes".into(), fixture.directory, true)
    );
    let associated: (i64, i64, i64, i64, i64, i64) = fixture
        .conn
        .query_row(
            "SELECT (SELECT count(*) FROM item_tags WHERE item_id=?1),
         (SELECT count(*) FROM album_items WHERE item_id=?1),
         (SELECT count(*) FROM document_versions WHERE item_id=?1 AND is_current=1),
         (SELECT count(*) FROM reader_bookmarks WHERE item_id=?1),
         (SELECT count(*) FROM reading_progress WHERE item_id=?1),
         (SELECT count(*) FROM reader_book_prefs WHERE item_id=?1)",
            [fixture.item],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(associated, (1, 1, 1, 1, 1, 1));
    let path = crate::utils::path::resolve_media_path(&parent.path, &directory.1, "note.md");
    assert_eq!(std::fs::read_to_string(path).unwrap(), "原始正文");
    assert_eq!(
        std::fs::read_to_string(fixture.temp.path().join("revision.txt")).unwrap(),
        "我的修订稿"
    );
    assert!(get_scan_root(&fixture.conn, fixture.root).is_err());
    assert!(get_scan_root(&fixture.conn, audio_root).is_err());
}

#[test]
fn merge_scan_roots_rejects_conflicts_and_rolls_back_write_failure() {
    let mut fixture = fixture();
    let target = insert_scan_root(&fixture.conn, &fixture.parent_path, None).unwrap();
    let before = snapshot(&fixture.conn);
    assert!(matches!(
        merge(&mut fixture),
        Err(AppError::Scan {
            code: "scan_merge_conflict",
            ..
        })
    ));
    assert_eq!(snapshot(&fixture.conn), before);
    delete_scan_root(&fixture.conn, target).unwrap();

    let nested = insert_scan_root(
        &fixture.conn,
        &format!("{}/collection/books/nested", fixture.parent_path),
        None,
    )
    .unwrap();
    set_scan_root_hidden(&fixture.conn, nested, true).unwrap();
    let sources = [
        source(fixture.root, "collection/books"),
        source(nested, "collection/books/nested"),
    ];
    let before = snapshot(&fixture.conn);
    assert!(matches!(
        merge_scan_root_children(
            &mut fixture.conn,
            &fixture.parent_path,
            None,
            &volume(),
            &sources
        ),
        Err(AppError::Scan {
            code: "scan_merge_visibility_conflict",
            ..
        })
    ));
    assert_eq!(snapshot(&fixture.conn), before);
    set_scan_root_hidden(&fixture.conn, nested, false).unwrap();
    let before = snapshot(&fixture.conn);
    assert!(matches!(
        merge_scan_root_children(
            &mut fixture.conn,
            &fixture.parent_path,
            None,
            &volume(),
            &sources
        ),
        Err(AppError::Scan {
            code: "scan_merge_conflict",
            ..
        })
    ));
    assert_eq!(snapshot(&fixture.conn), before);
    delete_scan_root(&fixture.conn, nested).unwrap();

    fixture.conn.execute(
        "INSERT INTO directory_move_journal (stage,source_dir_id,source_root_id,source_rel_path,source_abs_path,
         target_root_id,target_rel_path,target_abs_path,target_parent_id)
         VALUES ('source_leftover',?1,?2,'old',?3,?2,'new',?4,?1)",
        params![fixture.directory, fixture.root, format!("{}/old", fixture.parent_path), format!("{}/new", fixture.parent_path)],
    ).unwrap();
    let journal = fixture.conn.last_insert_rowid();
    let before = snapshot(&fixture.conn);
    assert!(matches!(
        merge(&mut fixture),
        Err(AppError::Scan {
            code: "scan_merge_pending_move",
            ..
        })
    ));
    assert_eq!(snapshot(&fixture.conn), before);
    fixture
        .conn
        .execute("DELETE FROM directory_move_journal WHERE id=?1", [journal])
        .unwrap();

    // 在目录迁移完成、删除旧空根时故障，证明已写入的父根、祖先链、卷与目录改归属均回滚。
    fixture
        .conn
        .execute_batch(
            "CREATE TRIGGER fail_merge BEFORE DELETE ON scan_roots
        BEGIN SELECT RAISE(ABORT, 'injected merge failure'); END;",
        )
        .unwrap();
    let before = snapshot(&fixture.conn);
    assert!(matches!(merge(&mut fixture), Err(AppError::Db(_))));
    assert_eq!(snapshot(&fixture.conn), before);
    assert_eq!(
        std::fs::read_to_string(
            std::path::Path::new(&fixture.parent_path).join("collection/books/note.md")
        )
        .unwrap(),
        "原始正文"
    );
}
