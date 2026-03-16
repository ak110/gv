use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use crossbeam_channel::Sender;

use crate::file_list::FileList;
use crate::image::{DecodedImage, ImageDecoder, StandardDecoder};

/// DocumentからUIへの通知イベント
#[derive(Debug)]
#[allow(dead_code)]
pub enum DocumentEvent {
    /// 画像のデコード完了、再描画可能
    ImageReady,
    /// ファイルリスト変更
    FileListChanged,
    /// 表示位置変更
    NavigationChanged { index: usize, count: usize },
    /// エラー通知
    Error(String),
}

/// 画像・ファイルリスト・状態管理（モデル層）
/// Win32 APIやHWNDへの依存は一切持たない
pub struct Document {
    event_sender: Sender<DocumentEvent>,
    decoder: StandardDecoder,
    current_image: Option<DecodedImage>,
    file_list: FileList,
}

impl Document {
    pub fn new(event_sender: Sender<DocumentEvent>) -> Self {
        Self {
            event_sender,
            decoder: StandardDecoder::new(),
            current_image: None,
            file_list: FileList::new(),
        }
    }

    /// ファイルを開く（親フォルダの画像を列挙し、指定ファイルを表示）
    pub fn open(&mut self, path: &Path) -> Result<()> {
        let path = Self::canonicalize(path)?;

        // 親フォルダの画像を列挙
        if let Some(folder) = path.parent() {
            self.file_list.populate_from_folder(folder)?;
            self.file_list.set_current_by_path(&path);
            let _ = self.event_sender.send(DocumentEvent::FileListChanged);
        }

        self.load_current()
    }

    /// フォルダを開く（先頭画像を表示）
    pub fn open_folder(&mut self, folder: &Path) -> Result<()> {
        let folder = Self::canonicalize(folder)?;
        self.file_list.populate_from_folder(&folder)?;

        if self.file_list.len() > 0 {
            self.file_list.navigate_first();
        }
        let _ = self.event_sender.send(DocumentEvent::FileListChanged);
        self.load_current()
    }

    /// 相対移動
    pub fn navigate_relative(&mut self, offset: isize) {
        if self.file_list.navigate_relative(offset) {
            let _ = self.load_current();
        }
    }

    /// 最初へ移動
    pub fn navigate_first(&mut self) {
        if self.file_list.navigate_first() {
            let _ = self.load_current();
        }
    }

    /// 最後へ移動
    pub fn navigate_last(&mut self) {
        if self.file_list.navigate_last() {
            let _ = self.load_current();
        }
    }

    /// 現在のファイルをデコードしてイベント送信
    fn load_current(&mut self) -> Result<()> {
        let path = match self.file_list.current() {
            Some(info) => info.path.clone(),
            None => {
                self.current_image = None;
                return Ok(());
            }
        };

        let data = std::fs::read(&path)
            .with_context(|| format!("ファイル読み込み失敗: {}", path.display()))?;

        match self.decoder.decode(&data) {
            Ok(image) => {
                self.current_image = Some(image);
                let _ = self.event_sender.send(DocumentEvent::ImageReady);
            }
            Err(e) => {
                self.current_image = None;
                let msg = format!("{}: {}", path.display(), e);
                let _ = self.event_sender.send(DocumentEvent::Error(msg));
            }
        }

        self.send_navigation_changed();
        Ok(())
    }

    /// NavigationChangedイベントを送信
    fn send_navigation_changed(&self) {
        if let Some(index) = self.file_list.current_index() {
            let _ = self.event_sender.send(DocumentEvent::NavigationChanged {
                index,
                count: self.file_list.len(),
            });
        }
    }

    /// パスを正規化する（相対パスやUNCパス対応）
    fn canonicalize(path: &Path) -> Result<PathBuf> {
        std::fs::canonicalize(path).with_context(|| format!("パス解決失敗: {}", path.display()))
    }

    /// 現在のデコード済み画像への参照
    pub fn current_image(&self) -> Option<&DecodedImage> {
        self.current_image.as_ref()
    }

    /// 現在のファイルパス
    pub fn current_path(&self) -> Option<&Path> {
        self.file_list.current().map(|f| f.path.as_path())
    }

    /// ファイルリストへの参照
    pub fn file_list(&self) -> &FileList {
        &self.file_list
    }
}
