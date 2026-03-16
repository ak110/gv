use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use crossbeam_channel::Sender;

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
    current_path: Option<PathBuf>,
}

impl Document {
    pub fn new(event_sender: Sender<DocumentEvent>) -> Self {
        Self {
            event_sender,
            decoder: StandardDecoder::new(),
            current_image: None,
            current_path: None,
        }
    }

    /// ファイルを開いてデコードする
    pub fn open(&mut self, path: &Path) -> Result<()> {
        let data = std::fs::read(path)
            .with_context(|| format!("ファイル読み込み失敗: {}", path.display()))?;

        match self.decoder.decode(&data) {
            Ok(image) => {
                self.current_image = Some(image);
                self.current_path = Some(path.to_path_buf());
                let _ = self.event_sender.send(DocumentEvent::ImageReady);
            }
            Err(e) => {
                let msg = format!("{}: {}", path.display(), e);
                let _ = self.event_sender.send(DocumentEvent::Error(msg));
            }
        }

        Ok(())
    }

    /// 現在のデコード済み画像への参照
    pub fn current_image(&self) -> Option<&DecodedImage> {
        self.current_image.as_ref()
    }

    /// 現在のファイルパス
    pub fn current_path(&self) -> Option<&Path> {
        self.current_path.as_deref()
    }
}
