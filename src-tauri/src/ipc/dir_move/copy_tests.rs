//! 目录复制的磁盘结果与取消边界；所有样本均在本次临时目录内。

use super::*;

#[test]
fn published_directory_copy_keeps_source_and_only_publishes_complete_tree() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let destination = temp.path().join("copy");
    std::fs::create_dir_all(source.join("nested")).unwrap();
    std::fs::write(source.join("one.txt"), b"one").unwrap();
    std::fs::write(source.join("nested/two.txt"), b"two").unwrap();

    assert_eq!(
        copy_tree_published(&source, &destination, "copy", None).unwrap(),
        2
    );
    assert_eq!(std::fs::read(source.join("one.txt")).unwrap(), b"one");
    assert_eq!(
        std::fs::read(destination.join("nested/two.txt")).unwrap(),
        b"two"
    );
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 2);

    std::fs::write(destination.join("one.txt"), b"keep").unwrap();
    assert!(matches!(
        copy_tree_published(&source, &destination, "copy", None),
        Err(AppError::DirectoryExists(_))
    ));
    assert_eq!(std::fs::read(destination.join("one.txt")).unwrap(), b"keep");
}

#[test]
fn directory_copy_cancels_inside_large_file_without_publishing() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let destination = temp.path().join("copy");
    std::fs::create_dir(&source).unwrap();
    let bytes = vec![0x5a; CHUNK * 64];
    std::fs::write(source.join("large.bin"), &bytes).unwrap();
    let cancel = CancellationToken::new();
    let mut progress = Vec::new();
    let mut report = |update: DirectoryCopyProgress| {
        if update.copied_bytes >= (CHUNK * 2) as u64 {
            cancel.cancel();
        }
        progress.push(update);
    };
    let mut control = DirectoryCopyControl::new(7, &cancel, &mut report);
    assert!(matches!(
        copy_tree_published(&source, &destination, "copy", Some(&mut control)),
        Err(AppError::Cancelled)
    ));
    assert_eq!(progress.first().unwrap().copied_bytes, 0);
    let last = progress.last().unwrap();
    assert_eq!(last.generation, 7);
    assert_eq!(last.copied_files, 0);
    assert_eq!(last.copied_bytes, (CHUNK * 2) as u64);
    assert!(progress.iter().all(|p| !p.publishing));
    assert!(!destination.exists());
    assert_eq!(std::fs::read(source.join("large.bin")).unwrap(), bytes);
    assert_eq!(
        std::fs::read_dir(temp.path()).unwrap().count(),
        1,
        "清除暂存与归属标记"
    );
}

#[test]
fn directory_copy_cancels_after_last_file_before_publish() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let destination = temp.path().join("copy");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("one.txt"), b"one").unwrap();
    let cancel = CancellationToken::new();
    let mut progress = Vec::new();
    let mut report = |update: DirectoryCopyProgress| {
        if update.copied_files == 1 {
            cancel.cancel();
        }
        progress.push(update);
    };
    let mut control = DirectoryCopyControl::new(8, &cancel, &mut report);
    assert!(matches!(
        copy_tree_published(&source, &destination, "copy", Some(&mut control)),
        Err(AppError::Cancelled)
    ));
    let last = progress.last().unwrap();
    assert_eq!((last.copied_files, last.copied_bytes), (1, 3));
    assert!(progress.iter().all(|p| !p.publishing));
    assert!(!destination.exists());
    assert_eq!(std::fs::read(source.join("one.txt")).unwrap(), b"one");
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
}

#[test]
fn directory_copy_late_cancel_preserves_published_success_and_progress() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let destination = temp.path().join("copy");
    std::fs::create_dir_all(source.join("nested")).unwrap();
    std::fs::write(source.join("one.txt"), b"one").unwrap();
    std::fs::write(source.join("nested/two.txt"), b"two!").unwrap();
    let cancel = CancellationToken::new();
    let mut progress = Vec::new();
    let mut report = |update: DirectoryCopyProgress| {
        if update.publishing {
            assert!(!destination.exists(), "发布通知发生在 rename 前");
            cancel.cancel();
        }
        progress.push(update);
    };
    let mut control = DirectoryCopyControl::new(9, &cancel, &mut report);
    assert_eq!(
        copy_tree_published(&source, &destination, "copy", Some(&mut control)).unwrap(),
        2
    );
    assert!(cancel.is_cancelled());
    assert!(progress
        .windows(2)
        .all(|p| p[0].copied_bytes <= p[1].copied_bytes && p[0].copied_files <= p[1].copied_files));
    assert!(progress
        .iter()
        .any(|p| p.copied_files == 1 && !p.publishing));
    assert_eq!(
        serde_json::to_value(progress.last().unwrap()).unwrap(),
        serde_json::json!({
            "generation": 9, "copiedFiles": 2, "copiedBytes": 7, "publishing": true
        })
    );
    assert_eq!(std::fs::read(source.join("one.txt")).unwrap(), b"one");
    assert_eq!(
        std::fs::read(source.join("nested/two.txt")).unwrap(),
        b"two!"
    );
    assert_eq!(std::fs::read(destination.join("one.txt")).unwrap(), b"one");
    assert_eq!(
        std::fs::read(destination.join("nested/two.txt")).unwrap(),
        b"two!"
    );
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 2);
}

#[test]
fn directory_copy_rejects_target_created_at_publish_boundary() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let destination = temp.path().join("copy");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("one.txt"), b"one").unwrap();
    let cancel = CancellationToken::new();
    let mut report = |update: DirectoryCopyProgress| {
        if update.publishing {
            std::fs::create_dir(&destination).unwrap();
        }
    };
    let mut control = DirectoryCopyControl::new(10, &cancel, &mut report);
    assert!(matches!(
        copy_tree_published(&source, &destination, "copy", Some(&mut control)),
        Err(AppError::CopyFile(_))
    ));
    assert_eq!(std::fs::read_dir(&destination).unwrap().count(), 0);
    assert_eq!(std::fs::read(source.join("one.txt")).unwrap(), b"one");
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 2);
}
