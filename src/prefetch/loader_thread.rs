use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;

use anyhow::{Context as _, Result};
use crossbeam_channel::{Receiver, Sender};

use crate::archive::ArchiveManager;
use crate::document::ZipBuffer;
use crate::image::{DecodedImage, DecoderChain};

/// ワーカースレッドへのリクエスト
enum LoadRequest {
    Load {
        index: usize,
        path: PathBuf,
        generation: u64,
        /// PDFページの場合: (pdf_path, page_index)
        pdf_page: Option<(PathBuf, u32)>,
        /// オンデマンドアーカイブエントリの場合: (archive_path, entry_name)
        archive_entry: Option<(PathBuf, String)>,
    },
    Shutdown,
}

/// ワーカースレッドからのレスポンス
pub enum LoadResponse {
    Loaded {
        index: usize,
        image: DecodedImage,
        generation: u64,
    },
    Failed {
        #[allow(dead_code)] // enum構造体フィールド: ログ出力等で参照可能にする
        index: usize,
        error: String,
        generation: u64,
    },
}

/// 先読みエンジン（ワーカースレッド管理）
pub struct PrefetchEngine {
    request_tx: Sender<LoadRequest>,
    response_rx: Receiver<LoadResponse>,
    worker_handle: Option<JoinHandle<()>>,
    /// ワーカーと共有する世代カウンタ
    current_generation: Arc<AtomicU64>,
    /// メインスレッド側のローカルコピー
    generation: u64,
}

impl PrefetchEngine {
    /// ワーカースレッドを起動する
    /// `notify`はレスポンス送信後に呼ばれるコールバック（UIスレッドへの通知用）
    /// `decoder`は画像デコーダチェーン（Susieプラグイン含む）
    pub fn new(
        notify: Box<dyn Fn() + Send>,
        decoder: Arc<DecoderChain>,
        archive_manager: Arc<ArchiveManager>,
        zip_buffers: Arc<RwLock<HashMap<PathBuf, ZipBuffer>>>,
    ) -> Result<Self> {
        let (request_tx, request_rx) = crossbeam_channel::unbounded();
        let (response_tx, response_rx) = crossbeam_channel::unbounded();
        let current_generation = Arc::new(AtomicU64::new(0));
        let gen_clone = Arc::clone(&current_generation);

        let worker_handle = std::thread::Builder::new()
            .name("prefetch-worker".to_string())
            .spawn(move || {
                worker_loop(
                    request_rx,
                    response_tx,
                    gen_clone,
                    notify,
                    decoder,
                    archive_manager,
                    zip_buffers,
                );
            })
            .context("先読みワーカースレッドの起動に失敗")?;

        Ok(Self {
            request_tx,
            response_rx,
            worker_handle: Some(worker_handle),
            current_generation,
            generation: 0,
        })
    }

    /// 現在のgenerationを付与してロードリクエストを送信
    pub fn request_load(
        &self,
        index: usize,
        path: PathBuf,
        pdf_page: Option<(PathBuf, u32)>,
        archive_entry: Option<(PathBuf, String)>,
    ) {
        let _ = self.request_tx.send(LoadRequest::Load {
            index,
            path,
            generation: self.generation,
            pdf_page,
            archive_entry,
        });
    }

    /// 全レスポンスをノンブロッキングで取得
    pub fn drain_responses(&self) -> Vec<LoadResponse> {
        let mut responses = Vec::new();
        while let Ok(resp) = self.response_rx.try_recv() {
            responses.push(resp);
        }
        responses
    }

    /// テスト用: 最初の1件をブロッキングで受信し、続けてノンブロッキングで残りを回収する。
    ///
    /// `recv_timeout` で確定的に待機するため、`thread::sleep` ベースのポーリングより
    /// 高速かつ flaky になりにくい。
    #[cfg(test)]
    pub fn recv_responses_blocking(&self, timeout: std::time::Duration) -> Vec<LoadResponse> {
        let mut responses = Vec::new();
        match self.response_rx.recv_timeout(timeout) {
            Ok(first) => responses.push(first),
            Err(_) => return responses,
        }
        while let Ok(resp) = self.response_rx.try_recv() {
            responses.push(resp);
        }
        responses
    }

    /// 現在の世代
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 世代を進行（AtomicU64も更新）
    pub fn advance_generation(&mut self) -> u64 {
        self.generation += 1;
        self.current_generation
            .store(self.generation, Ordering::Relaxed);
        self.generation
    }
}

impl Drop for PrefetchEngine {
    fn drop(&mut self) {
        let _ = self.request_tx.send(LoadRequest::Shutdown);
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
    }
}

/// COMの初期化/解放を管理するDropガード
struct ComGuard;

impl ComGuard {
    fn init() -> Self {
        unsafe {
            // ワーカースレッドではMTAモードで初期化
            let _ = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_MULTITHREADED,
            );
        }
        Self
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe {
            windows::Win32::System::Com::CoUninitialize();
        }
    }
}

/// ワーカースレッドのメインループ
fn worker_loop(
    request_rx: Receiver<LoadRequest>,
    response_tx: Sender<LoadResponse>,
    current_generation: Arc<AtomicU64>,
    notify: Box<dyn Fn() + Send>,
    decoder: Arc<DecoderChain>,
    archive_manager: Arc<ArchiveManager>,
    zip_buffers: Arc<RwLock<HashMap<PathBuf, ZipBuffer>>>,
) {
    // PDFレンダリングにWinRT APIが必要なのでCOM初期化
    let _com = ComGuard::init();

    while let Ok(request) = request_rx.recv() {
        match request {
            LoadRequest::Load {
                index,
                path,
                generation,
                pdf_page,
                archive_entry,
            } => {
                // デコード前に世代チェック → 古いリクエストはスキップ
                if generation < current_generation.load(Ordering::Relaxed) {
                    continue;
                }

                let response = if let Some((pdf_path, page_index)) = pdf_page {
                    // PDFページ: レンダリング
                    match crate::pdf_renderer::render_pdf_page(&pdf_path, page_index) {
                        Ok(image) => LoadResponse::Loaded {
                            index,
                            image,
                            generation,
                        },
                        Err(e) => LoadResponse::Failed {
                            index,
                            error: format!("{} page {}: {}", pdf_path.display(), page_index + 1, e),
                            generation,
                        },
                    }
                } else if let Some((archive_path, entry_name)) = archive_entry {
                    // オンデマンドアーカイブエントリ: バッファ→decode
                    let read_result = {
                        let buffers = zip_buffers.read().expect("zip_buffers lock poisoned");
                        if let Some(buffer) = buffers.get(&archive_path) {
                            crate::archive::zip::ZipHandler::read_entry_from_buffer(
                                buffer.as_ref(),
                                &entry_name,
                            )
                        } else {
                            drop(buffers);
                            archive_manager.read_entry(&archive_path, &entry_name)
                        }
                    };
                    let filename_hint = crate::archive::extract_filename(&entry_name).to_string();
                    match read_result {
                        Ok(data) => match decoder.decode(&data, &filename_hint) {
                            Ok(image) => LoadResponse::Loaded {
                                index,
                                image,
                                generation,
                            },
                            Err(e) => LoadResponse::Failed {
                                index,
                                error: format!(
                                    "{} > {}: {}",
                                    archive_path.display(),
                                    entry_name,
                                    e
                                ),
                                generation,
                            },
                        },
                        Err(e) => LoadResponse::Failed {
                            index,
                            error: format!("{} > {}: {}", archive_path.display(), entry_name, e),
                            generation,
                        },
                    }
                } else {
                    // 通常ファイル: fs::read → decode
                    let filename_hint = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    match std::fs::read(&path) {
                        Ok(data) => match decoder.decode(&data, filename_hint) {
                            Ok(image) => LoadResponse::Loaded {
                                index,
                                image,
                                generation,
                            },
                            Err(e) => LoadResponse::Failed {
                                index,
                                error: format!("{}: {}", path.display(), e),
                                generation,
                            },
                        },
                        Err(e) => LoadResponse::Failed {
                            index,
                            error: format!("{}: {}", path.display(), e),
                            generation,
                        },
                    }
                };

                let _ = response_tx.send(response);
                notify();
            }
            LoadRequest::Shutdown => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    use crate::image::StandardDecoder;

    fn test_decoder() -> Arc<DecoderChain> {
        Arc::new(DecoderChain::new(vec![Box::new(StandardDecoder::new())]))
    }

    fn test_archive_manager() -> Arc<ArchiveManager> {
        Arc::new(ArchiveManager::new(Arc::new(
            crate::extension_registry::ExtensionRegistry::new(),
        )))
    }

    fn test_zip_buffers() -> Arc<RwLock<HashMap<PathBuf, ZipBuffer>>> {
        Arc::new(RwLock::new(HashMap::new()))
    }

    /// テスト用: 1x1 白ピクセルのPNGバイナリを生成
    fn create_1x1_white_png() -> Vec<u8> {
        use image::{ImageBuffer, Rgba};
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(1, 1, Rgba([255, 255, 255, 255]));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        buf.into_inner()
    }

    #[test]
    fn load_and_receive_response() {
        let dir = std::env::temp_dir().join("gv_test_prefetch_load");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.png");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(&create_1x1_white_png()).unwrap();
        }

        let engine = PrefetchEngine::new(
            Box::new(|| {}),
            test_decoder(),
            test_archive_manager(),
            test_zip_buffers(),
        )
        .expect("test PrefetchEngine::new");
        engine.request_load(0, path, None, None);

        // ワーカーの処理完了を recv_timeout で確定的に待つ
        let responses = engine.recv_responses_blocking(std::time::Duration::from_secs(1));
        let mut loaded = false;
        for resp in responses {
            match resp {
                LoadResponse::Loaded { index, image, .. } => {
                    assert_eq!(index, 0);
                    assert_eq!(image.width, 1);
                    assert_eq!(image.height, 1);
                    loaded = true;
                }
                LoadResponse::Failed { error, .. } => {
                    panic!("unexpected failure: {error}");
                }
            }
        }
        assert!(loaded, "レスポンスが受信できなかった");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_generation_is_skipped() {
        let dir = std::env::temp_dir().join("gv_test_prefetch_gen");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.png");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(&create_1x1_white_png()).unwrap();
        }

        let mut engine = PrefetchEngine::new(
            Box::new(|| {}),
            test_decoder(),
            test_archive_manager(),
            test_zip_buffers(),
        )
        .expect("test PrefetchEngine::new");

        // generation=0でリクエストを送信する前に世代を進める
        engine.request_load(0, path.clone(), None, None);
        engine.advance_generation(); // → generation=1

        // generation=1で新しいリクエスト
        engine.request_load(1, path, None, None);

        // generation=1 のレスポンスが届くまで recv_timeout で確定的に待機する。
        // generation=0 のレスポンスはスキップされる可能性があるため has_gen1 まで繰り返す。
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut has_gen1 = false;
        while std::time::Instant::now() < deadline {
            let responses = engine.recv_responses_blocking(std::time::Duration::from_millis(200));
            if responses.is_empty() {
                continue;
            }
            if responses
                .iter()
                .any(|r| matches!(r, LoadResponse::Loaded { generation: 1, .. }))
            {
                has_gen1 = true;
                break;
            }
        }
        assert!(has_gen1, "generation=1のレスポンスが存在するべき");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_response_on_nonexistent_file() {
        let engine = PrefetchEngine::new(
            Box::new(|| {}),
            test_decoder(),
            test_archive_manager(),
            test_zip_buffers(),
        )
        .expect("test PrefetchEngine::new");
        engine.request_load(0, PathBuf::from("nonexistent_file_xyz.png"), None, None);

        let responses = engine.recv_responses_blocking(std::time::Duration::from_secs(1));
        let failed = responses
            .iter()
            .any(|resp| matches!(resp, LoadResponse::Failed { .. }));
        assert!(failed, "失敗レスポンスが受信できなかった");
    }

    #[test]
    fn drop_shuts_down_cleanly() {
        let engine = PrefetchEngine::new(
            Box::new(|| {}),
            test_decoder(),
            test_archive_manager(),
            test_zip_buffers(),
        )
        .expect("test PrefetchEngine::new");
        drop(engine);
        // パニックせずに終了すればOK
    }
}
