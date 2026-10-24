use std::path::{Path, PathBuf};

/// &str を null終端UTF-16ワイド文字列に変換する (Win32 API用)
pub fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `\\?\` プレフィックスを除去する (Shell API/WinRT APIが非対応のため)
/// UNCパスの場合: `\\?\UNC\server\share` → `\\server\share`
/// 通常パスの場合: `\\?\C:\path` → `C:\path`
pub fn strip_extended_length_prefix(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(unc) = s.strip_prefix(r"\\?\UNC\") {
        // UNCパス: \\?\UNC\server\share → \\server\share
        PathBuf::from(format!(r"\\{unc}"))
    } else if let Some(stripped) = s.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path.to_path_buf()
    }
}

/// 失敗の通知文を、要約、次の操作、詳細の順に1行へ組み立てる
///
/// `summary`と`next_action`は句点を付けずに渡す。詳細が長くても次の操作を先に読めるよう、詳細は末尾に置く。
pub fn format_error_text(summary: &str, next_action: &str, detail: Option<&str>) -> String {
    match detail {
        Some(detail) => format!("{summary}。{next_action}。詳細: {detail}"),
        None => format!("{summary}。{next_action}。"),
    }
}

/// 失敗を伝えるエラー用メッセージボックスを表示する
///
/// 利用者が次の行動を選べるよう、要約、次の操作、詳細を分けて受け取り、次の操作の指定を必須にする。
/// エラー用途のメッセージボックスはこの関数を通す。
/// `hwnd`は有効なウィンドウハンドルか`HWND::default()`（NULLに相当）を渡す。
pub fn show_error_box(
    hwnd: windows::Win32::Foundation::HWND,
    title: &str,
    summary: &str,
    next_action: &str,
    detail: Option<&str>,
) {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK};

    let message = match detail {
        Some(detail) => format!("{summary}。\n{next_action}。\n\n詳細:\n{detail}"),
        None => format!("{summary}。\n{next_action}。"),
    };
    // SAFETY: 呼出側は自身が所有する有効なウィンドウハンドルか`HWND::default()`を渡す（本関数の契約）。
    unsafe {
        show_message_box(hwnd, title, &message, MB_OK | MB_ICONERROR);
    }
}

/// Win32 MessageBoxW のラッパー
///
/// # Safety
/// hwnd は有効なウィンドウハンドル、または `HWND::default()`（NULLに相当）であること。
pub unsafe fn show_message_box(
    hwnd: windows::Win32::Foundation::HWND,
    title: &str,
    message: &str,
    flags: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE,
) -> windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_RESULT {
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::MessageBoxW(
            Some(hwnd),
            &windows::core::HSTRING::from(message),
            &windows::core::HSTRING::from(title),
            flags,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_text_puts_next_action_before_detail() {
        assert_eq!(
            format_error_text(
                "ファイルの移動に失敗しました",
                "移動先を確認してください",
                Some("拒否")
            ),
            "ファイルの移動に失敗しました。移動先を確認してください。詳細: 拒否"
        );
        assert_eq!(
            format_error_text("描画を復旧できませんでした", "再描画してください", None),
            "描画を復旧できませんでした。再描画してください。"
        );
    }

    #[test]
    fn empty_string() {
        let result = to_wide("");
        assert_eq!(result, vec![0]);
    }

    #[test]
    fn ascii_string() {
        let result = to_wide("hello");
        let expected: Vec<u16> = "hello".encode_utf16().chain(std::iter::once(0)).collect();
        assert_eq!(result, expected);
    }

    #[test]
    fn japanese_string() {
        let result = to_wide("画像ビューア");
        let expected: Vec<u16> = "画像ビューア"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        assert_eq!(result, expected);
        // null終端確認
        assert_eq!(*result.last().unwrap(), 0);
    }

    #[test]
    fn strip_prefix_local_path() {
        let path = Path::new(r"\\?\C:\Users\test\image.png");
        assert_eq!(
            strip_extended_length_prefix(path),
            PathBuf::from(r"C:\Users\test\image.png")
        );
    }

    #[test]
    fn strip_prefix_unc_path() {
        let path = Path::new(r"\\?\UNC\server\share\image.png");
        assert_eq!(
            strip_extended_length_prefix(path),
            PathBuf::from(r"\\server\share\image.png")
        );
    }

    #[test]
    fn strip_prefix_no_prefix() {
        let path = Path::new(r"C:\Users\test\image.png");
        assert_eq!(
            strip_extended_length_prefix(path),
            PathBuf::from(r"C:\Users\test\image.png")
        );
    }

    #[test]
    fn strip_prefix_plain_unc() {
        let path = Path::new(r"\\server\share\image.png");
        assert_eq!(
            strip_extended_length_prefix(path),
            PathBuf::from(r"\\server\share\image.png")
        );
    }
}
