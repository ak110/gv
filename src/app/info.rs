//! 画像情報とヘルプの表示

use super::AppWindow;
use crate::ui::info_dialog;

impl AppWindow {
    /// 数値を3桁カンマ区切りでフォーマットする
    pub(super) fn format_with_commas(n: u64) -> String {
        let s = n.to_string();
        let mut result = String::with_capacity(s.len() + s.len() / 3);
        for (i, c) in s.chars().enumerate() {
            if i > 0 && (s.len() - i).is_multiple_of(3) {
                result.push(',');
            }
            result.push(c);
        }
        result
    }

    /// 画像情報を表示する
    pub(super) fn show_image_info(&mut self) {
        let Some(info_lines) = self.build_image_info() else {
            return;
        };
        let text = info_lines.join(
            "

",
        );
        let font = self.monospace_font.hfont();
        let hwnd = self.hwnd;
        self.defer_modal(
            move || info_dialog::show_info_dialog(hwnd, "画像情報", &text, font),
            |app, result| {
                app.take_success("画像情報ダイアログの表示", result.map(Some));
            },
        );
    }

    /// 画像情報の表示行を組み立てる。メタデータ取得の失敗はタイトルバーへ通知し、取得済みの基本情報は返す
    pub(super) fn build_image_info(&mut self) -> Option<Vec<String>> {
        let source = self.document.current_source()?;
        let file_info = self.document.file_list().current()?;

        let mut info_lines = Vec::new();
        info_lines.push(format!("パス: {}", source.display_path()));
        info_lines.push(format!(
            "ファイルサイズ: {} KiB",
            Self::format_with_commas(file_info.file_size / 1024)
        ));

        if let Some(img) = self.document.current_image() {
            info_lines.push(format!("画像サイズ: {} x {}", img.width, img.height));
        }

        // メタデータ取得 (デコーダ経由)
        match self.document.current_metadata() {
            Ok(metadata) => {
                info_lines.push(format!("フォーマット: {}", metadata.format));
                for comment in &metadata.comments {
                    info_lines.push(comment.clone());
                }
                // EXIF情報
                if !metadata.exif.is_empty() {
                    info_lines.push(String::new());
                    info_lines.push("--- EXIF ---".to_string());
                    for (key, value) in &metadata.exif {
                        info_lines.push(format!("{key}: {value}"));
                    }
                }
            }
            Err(e) => {
                let msg = format!("メタデータの取得に失敗しました: {e:#}");
                info_lines.push(msg.clone());
                self.show_error_title(&msg);
            }
        }
        Some(info_lines)
    }

    /// ヘルプを表示する
    pub(super) fn show_help(&mut self) {
        let text = crate::help::build_help(&self.key_config);

        let font = self.monospace_font.hfont();
        let hwnd = self.hwnd;
        self.defer_modal(
            move || info_dialog::show_info_dialog(hwnd, "ぐらびゅ ヘルプ", &text, font),
            |app, result| {
                app.take_success("ヘルプの表示", result.map(Some));
            },
        );
    }
}
