//! 編集セッション管理
//!
//! 画像の編集状態を管理する。元画像のバックアップを保持し、
//! 編集済み画像を提供する。F5（再読み込み）で元画像に復元可能。

use crate::image::DecodedImage;

/// 編集セッション
pub struct EditingSession {
    /// 元画像のバックアップ（復元用）
    _original: DecodedImage,
    /// 編集が行われたかどうか
    modified: bool,
}

impl EditingSession {
    /// 新しい編集セッションを開始する
    /// `original` は編集前の画像のクローン
    pub fn new(original: DecodedImage) -> Self {
        Self {
            _original: original,
            modified: false,
        }
    }

    /// 編集済みフラグを設定する
    pub fn mark_modified(&mut self) {
        self.modified = true;
    }

    /// 未保存の編集があるかどうか
    pub fn has_unsaved_changes(&self) -> bool {
        self.modified
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_image() -> DecodedImage {
        DecodedImage {
            data: vec![255, 0, 0, 255],
            width: 1,
            height: 1,
        }
    }

    #[test]
    fn new_session_not_modified() {
        let session = EditingSession::new(dummy_image());
        assert!(!session.has_unsaved_changes());
    }

    #[test]
    fn mark_modified_sets_flag() {
        let mut session = EditingSession::new(dummy_image());
        session.mark_modified();
        assert!(session.has_unsaved_changes());
    }
}
