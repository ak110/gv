//! Document モジュールの型定義
//! ZIPバッファ、イベント、コンテナ結果など、Document全体で使う各種型を集約する。

use std::path::PathBuf;

use crate::archive::ArchiveImageEntry;

/// ZIPファイルのバッファ (mmapまたはメモリ読み込み)
#[derive(Debug)]
pub enum ZipBuffer {
    /// メモリマップドファイル (OSがページフォルト駆動で必要部分のみロード)
    Mmap(memmap2::Mmap),
    /// ヒープ上のバイト列 (mmapフォールバック用)
    Memory(Vec<u8>),
}

impl AsRef<[u8]> for ZipBuffer {
    fn as_ref(&self) -> &[u8] {
        match self {
            ZipBuffer::Mmap(m) => m,
            ZipBuffer::Memory(v) => v,
        }
    }
}

/// Documentが構築し、appモジュールが受信するUI通知イベント
#[derive(Debug)]
pub enum DocumentEvent {
    /// 現在画像の状態が変わり、画像領域の再描画が必要
    /// (デコード完了と編集結果の適用のほか、未展開コンテナへの移動や一覧の消去で現在画像を空にした場合も送る)
    ImageReady,
    /// ファイルリスト変更
    FileListChanged,
    /// 表示位置変更
    NavigationChanged { index: usize },
    /// エラー通知
    Error(ErrorNotice),
}

/// 利用者へ通知する失敗
///
/// 利用者が次の行動を選べるよう、要約、次の操作、詳細を分けて持ち、次の操作の指定を必須にする。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorNotice {
    /// 失敗の要約（句点なし）
    pub summary: String,
    /// 続行の可否、再操作、代替手段などの次の操作（句点なし）
    pub next_action: &'static str,
    /// 原因などの詳細
    pub detail: Option<String>,
}

impl ErrorNotice {
    pub fn new(summary: impl Into<String>, next_action: &'static str) -> Self {
        Self {
            summary: summary.into(),
            next_action,
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// コンテナ (ZIP/PDF/RAR/7z) の読み込み結果 (同期展開とバックグラウンド展開で共通、失敗を含む)
pub(super) enum ContainerResult {
    Pdf {
        path: PathBuf,
        page_count: u32,
    },
    Zip {
        path: PathBuf,
        buffer: ZipBuffer,
        entries: Vec<ArchiveImageEntry>,
    },
    TempExtracted {
        path: PathBuf,
        temp_dir: PathBuf,
        entries: Vec<crate::archive::ExtractedEntry>,
    },
    Error {
        path: PathBuf,
        error: String,
    },
}

/// コンテナの展開状態
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContainerState {
    /// 未展開 (プレースホルダとしてリストに存在、バックグラウンドキュー未投入)
    Pending,
    /// バックグラウンド展開中
    InFlight,
    /// 展開処理済み (展開に失敗してリストから除いた場合を含む)
    Expanded,
}

/// バックグラウンド展開スレッドからの通知イベント
pub(super) enum ContainerExpandEvent {
    /// 1コンテナの展開結果 (失敗時は`ContainerResult::Error`を保持する)
    Expanded {
        container_path: PathBuf,
        result: ContainerResult,
        generation: u64,
    },
    /// 展開ジョブの終了 (取消やスレッドプール作成失敗で未処理のコンテナが残る場合も送る)
    AllDone { generation: u64 },
}
