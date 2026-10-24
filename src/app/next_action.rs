//! 失敗の通知で利用者へ示す次の操作
//!
//! 文面は句点を付けずに書く。案内するメニュー項目名は`ui::menu`の表示名と一致させる。

/// 描画の失敗
pub(super) const RENDER_RETRY: &str = "ウィンドウサイズの変更などで再描画すると再試行します。改善しない場合はぐらびゅを再起動してください";
/// ファイル・フォルダを開けなかった
pub(super) const OPEN_RETRY: &str = "パスと読み取り権限を確認し、「ファイルを開く」「フォルダを開く」かドロップで開き直してください";
/// ダイアログを表示できなかった
pub(super) const DIALOG_RETRY: &str =
    "同じ操作をもう一度実行してください。改善しない場合はぐらびゅを再起動してください";
pub(super) const BOOKMARK_LOAD: &str =
    "ブックマークファイルの所在と内容を確認し、「ブックマーク読み込み」をもう一度実行してください";
pub(super) const BOOKMARK_SAVE: &str =
    "保存先のフォルダと書き込み権限を確認し、「ブックマーク保存」をもう一度実行してください";
pub(super) const IMAGE_EXPORT: &str =
    "保存先のフォルダ、空き容量、書き込み権限を確認し、もう一度保存してください";
pub(super) const FILE_DELETE: &str =
    "ファイルを開いている他のアプリを閉じ、「ファイルを削除」をもう一度実行してください";
pub(super) const FILE_MOVE: &str = "移動先のフォルダと書き込み権限、ファイルを開いている他のアプリが無いかを確認し、「ファイルを移動」をもう一度実行してください";
pub(super) const FILE_COPY: &str = "複製先のフォルダ、空き容量、書き込み権限を確認し、「ファイルを複製」をもう一度実行してください";
pub(super) const MARKED_DELETE: &str =
    "ファイルを開いている他のアプリを閉じ、「マークを完全に削除」をもう一度実行してください";
pub(super) const MARKED_COPY: &str =
    "複製先のフォルダ、空き容量、書き込み権限を確認し、「マークを複製」をもう一度実行してください";
pub(super) const MARKED_MOVE: &str = "移動先のフォルダと書き込み権限、ファイルを開いている他のアプリが無いかを確認し、「マークを移動」をもう一度実行してください";
pub(super) const MARKED_COPY_NAMES: &str = "他のアプリがクリップボードを使用中でないか確認し、「マークのファイル名をコピー」をもう一度実行してください";
pub(super) const COPY_FILE_NAME: &str = "他のアプリがクリップボードを使用中でないか確認し、「ファイル名をコピー」をもう一度実行してください";
pub(super) const COPY_IMAGE: &str =
    "他のアプリがクリップボードを使用中でないか確認し、「画像をコピー」をもう一度実行してください";
pub(super) const PASTE_READ: &str = "画像をクリップボードへコピーし直して「クリップボードから貼り付け」をもう一度実行するか、画像をファイルに保存して「ファイルを開く」で開いてください";
pub(super) const PASTE_SAVE: &str = "一時フォルダ（「一時フォルダ」で開ける）の空き容量と書き込み権限を確認し、もう一度貼り付けてください";
pub(super) const PASTE_OPEN: &str = "「クリップボードから貼り付け」をもう一度実行してください";
pub(super) const RESIZE: &str = "幅と高さを変えて「解像度の変更」をもう一度実行してください";
pub(super) const FILTER_INPUT: &str = "詳細に示す条件に合う値を入力し、もう一度実行してください";
pub(super) const METADATA: &str = "画像の表示と他の画像情報は使えるため対応は不要です";
pub(super) const HELP: &str =
    "もう一度実行するか、コマンドプロンプトで「ぐらびゅ.exe --help」を実行してください";
pub(super) const OPEN_DIALOG: &str =
    "もう一度実行するか、ファイルをウィンドウへドロップしてください";
pub(super) const NEW_WINDOW: &str =
    "「新規ウィンドウ」をもう一度実行するか、ぐらびゅ.exeを直接起動してください";
pub(super) const EXPLORER: &str = "エクスプローラーを起動して対象のフォルダを直接開いてください";
pub(super) const FOLDER_CREATE: &str =
    "フォルダの作成先への書き込み権限を確認し、同じメニューをもう一度実行してください";
pub(super) const BROWSER: &str = "https://github.com/ak110/gv をブラウザで直接開いてください";
pub(super) const PREFETCH_START: &str = "画像の表示は続けられますが、切り替えが遅くなります。改善しない場合はぐらびゅを再起動してください";
pub(super) const SKIPPED_COPY: &str =
    "未展開コンテナは表示して展開した後にもう一度実行してください";
