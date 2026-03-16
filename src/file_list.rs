use std::path::Path;
use std::sync::Arc;

use anyhow::Result;

use crate::extension_registry::ExtensionRegistry;
use crate::file_info::FileInfo;

/// ソート順
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum SortOrder {
    /// ファイル名順
    Name,
    /// ファイル名順（大文字小文字区別なし）
    NameNoCase,
    /// ファイルサイズ順
    Size,
    /// 最終更新日時順
    Date,
    /// 自然順ソート（数値認識）
    Natural,
}

/// ファイル一覧管理
pub struct FileList {
    files: Vec<FileInfo>,
    current_index: Option<usize>,
    sort_order: SortOrder,
    registry: Arc<ExtensionRegistry>,
}

impl FileList {
    pub fn new(registry: Arc<ExtensionRegistry>) -> Self {
        Self {
            files: Vec::new(),
            current_index: None,
            sort_order: SortOrder::Natural,
            registry,
        }
    }

    /// フォルダ内の画像ファイルを列挙してリストを構築する
    pub fn populate_from_folder(&mut self, folder: &Path) -> Result<()> {
        self.files.clear();
        self.current_index = None;

        let entries = std::fs::read_dir(folder)?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            // 拡張子フィルタ（ExtensionRegistry経由）
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && self.registry.is_image_extension(name)
                && let Ok(info) = FileInfo::from_path(&path)
            {
                self.files.push(info);
            }
        }

        self.sort(self.sort_order);
        Ok(())
    }

    /// パスで現在位置を設定する
    pub fn set_current_by_path(&mut self, path: &Path) -> bool {
        if let Some(idx) = self.files.iter().position(|f| f.path == path) {
            self.current_index = Some(idx);
            true
        } else {
            false
        }
    }

    /// 相対移動（ラップアラウンド）
    /// 先頭で後退 → 末尾へ、末尾で前進 → 先頭へ
    /// load_failedのファイルは同方向にスキップする
    pub fn navigate_relative(&mut self, offset: isize) -> bool {
        let len = self.files.len();
        if len == 0 {
            return false;
        }
        let current = self.current_index.unwrap_or(0);

        // ラップアラウンド付きでtarget計算
        let mut target = ((current as isize + offset) % len as isize + len as isize) as usize % len;

        // スキップ方向（offsetの符号に合わせる）
        let step: isize = if offset >= 0 { 1 } else { -1 };

        // load_failedのファイルを同方向にスキップ（最大len回で打ち切り）
        let mut attempts = 0;
        while self.files[target].load_failed && attempts < len {
            target = ((target as isize + step) % len as isize + len as isize) as usize % len;
            attempts += 1;
        }
        if attempts >= len {
            // 全てfailedで移動先がない
            return false;
        }

        if target != current {
            self.current_index = Some(target);
            true
        } else {
            false
        }
    }

    /// 指定インデックスへ移動
    pub fn navigate_to(&mut self, index: usize) -> bool {
        if index < self.files.len() && self.current_index != Some(index) {
            self.current_index = Some(index);
            true
        } else {
            false
        }
    }

    /// 最初へ移動
    pub fn navigate_first(&mut self) -> bool {
        self.navigate_to(0)
    }

    /// 最後へ移動
    pub fn navigate_last(&mut self) -> bool {
        if self.files.is_empty() {
            return false;
        }
        self.navigate_to(self.files.len() - 1)
    }

    /// ソート実行（現在位置をパスで維持）
    pub fn sort(&mut self, order: SortOrder) {
        // 現在のパスを記憶
        let current_path = self.current().map(|f| f.path.clone());

        match order {
            SortOrder::Name => {
                self.files.sort_by(|a, b| a.file_name.cmp(&b.file_name));
            }
            SortOrder::NameNoCase => {
                self.files
                    .sort_by(|a, b| a.file_name.to_lowercase().cmp(&b.file_name.to_lowercase()));
            }
            SortOrder::Size => {
                self.files.sort_by_key(|f| f.file_size);
            }
            SortOrder::Date => {
                self.files.sort_by_key(|f| f.modified);
            }
            SortOrder::Natural => {
                self.files
                    .sort_by(|a, b| natord::compare(&a.file_name, &b.file_name));
            }
        }

        self.sort_order = order;

        // パスで位置を復元
        if let Some(path) = current_path {
            self.set_current_by_path(&path);
        }
    }

    /// 指定インデックスのファイルをデコード失敗状態にする
    pub fn mark_failed(&mut self, index: usize) {
        if let Some(info) = self.files.get_mut(index) {
            info.load_failed = true;
        }
    }

    /// 全ファイルの失敗状態をクリア（フォルダ再読み込み時用）
    pub fn clear_failed(&mut self) {
        for info in &mut self.files {
            info.load_failed = false;
        }
    }

    /// ファイル一覧への参照
    pub fn files(&self) -> &[FileInfo] {
        &self.files
    }

    /// 現在のファイル情報
    pub fn current(&self) -> Option<&FileInfo> {
        self.current_index.and_then(|i| self.files.get(i))
    }

    /// ファイル数
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// 現在のインデックス
    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn test_registry() -> Arc<ExtensionRegistry> {
        Arc::new(ExtensionRegistry::new())
    }

    /// テスト用のダミー画像ファイルを作成するヘルパー
    fn create_test_files(dir: &Path, names: &[&str]) {
        let _ = std::fs::create_dir_all(dir);
        for name in names {
            let mut f = std::fs::File::create(dir.join(name)).unwrap();
            f.write_all(b"dummy").unwrap();
        }
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn populate_filters_by_extension() {
        let dir = std::env::temp_dir().join("gv3_test_fl_populate");
        create_test_files(&dir, &["a.jpg", "b.png", "c.txt", "d.bmp", "readme.md"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();

        assert_eq!(fl.len(), 3); // jpg, png, bmp
        cleanup(&dir);
    }

    #[test]
    fn natural_sort_order() {
        let dir = std::env::temp_dir().join("gv3_test_fl_natural");
        create_test_files(&dir, &["img2.png", "img10.png", "img1.png"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();
        fl.sort(SortOrder::Natural);

        let names: Vec<&str> = fl.files.iter().map(|f| f.file_name.as_str()).collect();
        assert_eq!(names, vec!["img1.png", "img2.png", "img10.png"]);
        cleanup(&dir);
    }

    #[test]
    fn navigate_relative_wraps_around() {
        let dir = std::env::temp_dir().join("gv3_test_fl_nav");
        create_test_files(&dir, &["a.png", "b.png", "c.png"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();
        fl.navigate_to(0);

        // 先頭で後退 → 末尾へラップ
        assert!(fl.navigate_relative(-1));
        assert_eq!(fl.current_index(), Some(2));

        // 末尾で前進 → 先頭へラップ
        assert!(fl.navigate_relative(1));
        assert_eq!(fl.current_index(), Some(0));

        // 通常の相対移動
        fl.navigate_to(1);
        assert!(fl.navigate_relative(1));
        assert_eq!(fl.current_index(), Some(2));

        cleanup(&dir);
    }

    #[test]
    fn navigate_first_last() {
        let dir = std::env::temp_dir().join("gv3_test_fl_firstlast");
        create_test_files(&dir, &["a.png", "b.png", "c.png"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();
        fl.navigate_to(1);

        fl.navigate_last();
        assert_eq!(fl.current_index(), Some(2));

        fl.navigate_first();
        assert_eq!(fl.current_index(), Some(0));

        cleanup(&dir);
    }

    #[test]
    fn set_current_by_path_found() {
        let dir = std::env::temp_dir().join("gv3_test_fl_setpath");
        create_test_files(&dir, &["a.png", "b.png"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();

        assert!(fl.set_current_by_path(&dir.join("b.png")));
        assert_eq!(fl.current().unwrap().file_name, "b.png");

        cleanup(&dir);
    }

    #[test]
    fn sort_preserves_current_position() {
        let dir = std::env::temp_dir().join("gv3_test_fl_sortpreserve");
        create_test_files(&dir, &["c.png", "a.png", "b.png"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();
        fl.set_current_by_path(&dir.join("b.png"));

        fl.sort(SortOrder::Name);
        // ソート後もb.pngが選択されている
        assert_eq!(fl.current().unwrap().file_name, "b.png");

        cleanup(&dir);
    }

    #[test]
    fn empty_list_navigation() {
        let mut fl = FileList::new(test_registry());
        assert!(!fl.navigate_relative(1));
        assert!(!fl.navigate_first());
        assert!(!fl.navigate_last());
        assert!(fl.current().is_none());
    }
}
