use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;

use anyhow::{Context as _, Result};
use crossbeam_channel::{Receiver, Sender};

use crate::archive::ArchiveManager;
use crate::document::ZipBuffer;
use crate::file_info::FileSource;
use crate::image::{DecodedImage, DecoderChain};
use crate::persistent_filter::PersistentFilter;

/// ワーカースレッドへのリクエスト
enum LoadRequest {
    Load {
        index: usize,
        source: FileSource,
        filter: PersistentFilter,
        generation: u64,
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
        error: String,
        generation: u64,
    },
}

/// 先読みエンジン (ワーカースレッド管理)
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
    /// `notify`はレスポンス送信後に呼ばれるコールバック (UIスレッドへの通知用)
    /// `decoder`は画像デコーダチェーン (Susieプラグイン含む)
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
    pub fn request_load(&self, index: usize, source: FileSource, filter: PersistentFilter) {
        let _ = self.request_tx.send(LoadRequest::Load {
            index,
            source,
            filter,
            generation: self.generation,
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

    /// 世代を進行 (AtomicU64も更新)
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
                source,
                filter,
                generation,
            } => {
                // デコード前に世代チェック → 古いリクエストはスキップ
                if generation < current_generation.load(Ordering::Relaxed) {
                    continue;
                }

                let response = match crate::image::decode_source(
                    &source,
                    &decoder,
                    &archive_manager,
                    &zip_buffers,
                    false,
                ) {
                    Ok(image) => LoadResponse::Loaded {
                        index,
                        image: filter.apply(&image).unwrap_or(image),
                        generation,
                    },
                    Err(error) => LoadResponse::Failed {
                        error: format!("{}: {error}", source.display_path()),
                        generation,
                    },
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
    use crate::test_helpers::{
        create_1x1_white_png, test_archive_manager, test_decoder, test_zip_buffers,
    };
    use std::io::Write;

    #[test]
    fn load_and_receive_response() {
        let dir = crate::test_helpers::TempDir::new("prefetch_load");
        let path = dir.join("test.png");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(&create_1x1_white_png()).unwrap();
        }

        let engine = PrefetchEngine::new(
            Box::new(|| {}),
            test_decoder(),
            Arc::new(test_archive_manager(&Arc::new(
                crate::extension_registry::ExtensionRegistry::new(),
            ))),
            test_zip_buffers(),
        )
        .expect("test PrefetchEngine::new");
        engine.request_load(0, FileSource::File(path), PersistentFilter::new());

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
    }

    #[test]
    fn stale_generation_is_skipped() {
        let dir = crate::test_helpers::TempDir::new("prefetch_gen");
        let path = dir.join("test.png");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(&create_1x1_white_png()).unwrap();
        }

        let mut engine = PrefetchEngine::new(
            Box::new(|| {}),
            test_decoder(),
            Arc::new(test_archive_manager(&Arc::new(
                crate::extension_registry::ExtensionRegistry::new(),
            ))),
            test_zip_buffers(),
        )
        .expect("test PrefetchEngine::new");

        // generation=0でリクエストを送信する前に世代を進める
        engine.request_load(0, FileSource::File(path.clone()), PersistentFilter::new());
        engine.advance_generation(); // → generation=1

        // generation=1で新しいリクエスト
        engine.request_load(1, FileSource::File(path), PersistentFilter::new());

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
    }

    #[test]
    fn failed_response_on_nonexistent_file() {
        let engine = PrefetchEngine::new(
            Box::new(|| {}),
            test_decoder(),
            Arc::new(test_archive_manager(&Arc::new(
                crate::extension_registry::ExtensionRegistry::new(),
            ))),
            test_zip_buffers(),
        )
        .expect("test PrefetchEngine::new");
        engine.request_load(
            0,
            FileSource::File(PathBuf::from("nonexistent_file_xyz.png")),
            PersistentFilter::new(),
        );

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
            Arc::new(test_archive_manager(&Arc::new(
                crate::extension_registry::ExtensionRegistry::new(),
            ))),
            test_zip_buffers(),
        )
        .expect("test PrefetchEngine::new");
        drop(engine);
        // パニックせずに終了すればOK
    }
    #[test]
    fn synchronous_and_prefetch_use_original_hint_and_request_filter() {
        struct HintDecoder(Arc<std::sync::Mutex<Vec<String>>>);
        impl crate::image::ImageDecoder for HintDecoder {
            fn can_decode(&self, _data: &[u8], _hint: &str) -> bool {
                true
            }
            fn decode(&self, _data: &[u8], hint: &str) -> Result<DecodedImage> {
                self.0.lock().unwrap().push(hint.into());
                Ok(DecodedImage {
                    data: vec![10, 20, 30, 255],
                    width: 1,
                    height: 1,
                })
            }
            fn metadata(&self, _data: &[u8], _hint: &str) -> Result<crate::image::ImageMetadata> {
                anyhow::bail!("試験ではメタデータを取得しない")
            }
        }
        let dir = crate::test_helpers::TempDir::new("prefetch_source_hint");
        let path = dir.join("actual.png");
        let temp = dir.join("unrelated.tmp");
        std::fs::write(&path, b"image").unwrap();
        std::fs::write(&temp, b"image").unwrap();
        let archive = dir.join("images.zip");
        let mut writer = ::zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        writer
            .start_file(
                "folder/actual.png",
                ::zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(b"image").unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        let buffers = test_zip_buffers();
        buffers
            .write()
            .unwrap()
            .insert(archive.clone(), ZipBuffer::Memory(bytes));
        let hints = Arc::new(std::sync::Mutex::new(Vec::new()));
        let decoder = Arc::new(DecoderChain::new(vec![Box::new(HintDecoder(Arc::clone(
            &hints,
        )))]));
        let manager = Arc::new(test_archive_manager(&Arc::new(
            crate::extension_registry::ExtensionRegistry::new(),
        )));
        let engine = PrefetchEngine::new(
            Box::new(|| {}),
            Arc::clone(&decoder),
            Arc::clone(&manager),
            Arc::clone(&buffers),
        )
        .unwrap();
        let sources = [
            FileSource::File(path),
            FileSource::ArchiveEntry {
                archive: archive.clone(),
                entry: "folder/actual.png".into(),
                on_demand: false,
                temp_path: Some(temp),
                entry_index: None,
            },
            FileSource::ArchiveEntry {
                archive,
                entry: "folder/actual.png".into(),
                on_demand: true,
                temp_path: None,
                entry_index: Some(0),
            },
        ];
        for (index, source) in sources.into_iter().enumerate() {
            let image =
                crate::image::decode_source(&source, &decoder, &manager, &buffers, true).unwrap();
            assert_eq!(image.data, vec![10, 20, 30, 255]);
            let mut filter = PersistentFilter::new();
            filter.toggle_enabled();
            filter.add_operation(crate::persistent_filter::FilterOperation::InvertColors);
            engine.request_load(index, source, filter.clone());
            filter.toggle_enabled();
            let responses = engine.recv_responses_blocking(std::time::Duration::from_secs(5));
            assert_eq!(responses.len(), 1);
            match &responses[0] {
                LoadResponse::Loaded { image, .. } => {
                    assert_eq!(image.data, vec![245, 235, 225, 255]);
                }
                LoadResponse::Failed { error, .. } => panic!("{error}"),
            }
        }
        assert_eq!(*hints.lock().unwrap(), vec!["actual.png"; 6]);
        drop(engine);
        let _ = std::fs::remove_dir_all(dir);
    }
}
