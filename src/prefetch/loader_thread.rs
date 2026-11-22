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

enum LoadRequest {
    Load {
        index: usize,
        source: FileSource,
        filter: PersistentFilter,
        generation: u64,
    },
    Shutdown,
}

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

pub struct PrefetchEngine {
    request_tx: Sender<LoadRequest>,
    response_rx: Receiver<LoadResponse>,
    worker_handle: Option<JoinHandle<()>>,
    current_generation: Arc<AtomicU64>,
    generation: u64,
}

impl PrefetchEngine {
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

    pub fn request_load(&self, index: usize, source: FileSource, filter: PersistentFilter) {
        let _ = self.request_tx.send(LoadRequest::Load {
            index,
            source,
            filter,
            generation: self.generation,
        });
    }

    pub fn drain_responses(&self) -> Vec<LoadResponse> {
        let mut responses = Vec::new();
        while let Ok(resp) = self.response_rx.try_recv() {
            responses.push(resp);
        }
        responses
    }

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

    pub fn generation(&self) -> u64 {
        self.generation
    }

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

struct ComGuard;

impl ComGuard {
    fn init() -> Self {
        unsafe {
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
        std::fs::write(&path, create_1x1_white_png()).unwrap();
        let (notify, notified) = crossbeam_channel::unbounded();
        let mut engine = PrefetchEngine::new(
            Box::new(move || {
                let _ = notify.send(());
            }),
            test_decoder(),
            Arc::new(test_archive_manager(&Arc::new(
                crate::extension_registry::ExtensionRegistry::new(),
            ))),
            test_zip_buffers(),
        )
        .unwrap();
        engine.advance_generation();
        engine.request_load(7, FileSource::File(path), PersistentFilter::new());
        notified
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let rx = engine.response_rx.clone();
        drop(engine);
        let responses: Vec<_> = rx.try_iter().collect();
        let [
            LoadResponse::Loaded {
                index: 7,
                generation: 1,
                image,
            },
        ] = responses.as_slice()
        else {
            panic!("要求に対応する成功応答が1件必要");
        };
        assert_eq!((image.width, image.height), (1, 1));
        assert_eq!(image.data, [255; 4]);
    }

    #[test]
    fn stale_generation_is_skipped() {
        use std::time::Duration;
        let dir = crate::test_helpers::TempDir::new("prefetch_gen");
        let path = dir.join("test.png");
        std::fs::write(&path, create_1x1_white_png()).unwrap();
        let (ready, entered) = crossbeam_channel::unbounded();
        let (resume, resumed) = crossbeam_channel::bounded(1);
        let timeout = Duration::from_secs(5);
        let first = std::sync::atomic::AtomicBool::new(true);
        let mut engine = PrefetchEngine::new(
            Box::new(move || {
                if first.swap(false, Ordering::Relaxed) {
                    let _ = ready.send(());
                    resumed.recv_timeout(timeout).unwrap();
                }
            }),
            test_decoder(),
            Arc::new(test_archive_manager(&Arc::new(
                crate::extension_registry::ExtensionRegistry::new(),
            ))),
            test_zip_buffers(),
        )
        .unwrap();
        engine.request_load(0, FileSource::File(path.clone()), PersistentFilter::new());
        entered.recv_timeout(timeout).unwrap();
        engine.request_load(1, FileSource::File(path.clone()), PersistentFilter::new());
        engine.advance_generation();
        engine.request_load(2, FileSource::File(path), PersistentFilter::new());
        resume.send(()).unwrap();
        let rx = engine.response_rx.clone();
        drop(engine);
        let responses: Vec<_> = rx.try_iter().collect();
        assert!(matches!(
            responses.as_slice(),
            [
                LoadResponse::Loaded {
                    index: 0,
                    generation: 0,
                    ..
                },
                LoadResponse::Loaded {
                    index: 2,
                    generation: 1,
                    ..
                },
            ]
        ));
    }

    #[test]
    fn failed_response_on_nonexistent_file() {
        let dir = crate::test_helpers::TempDir::new("prefetch_missing");
        let path = dir.join("missing.png");
        let mut engine = PrefetchEngine::new(
            Box::new(|| {}),
            test_decoder(),
            Arc::new(test_archive_manager(&Arc::new(
                crate::extension_registry::ExtensionRegistry::new(),
            ))),
            test_zip_buffers(),
        )
        .expect("test PrefetchEngine::new");
        engine.advance_generation();
        engine.request_load(0, FileSource::File(path.clone()), PersistentFilter::new());

        let responses = engine.recv_responses_blocking(std::time::Duration::from_secs(1));
        let [
            LoadResponse::Failed {
                error,
                generation: 1,
            },
        ] = responses.as_slice()
        else {
            panic!("不在ファイルに対応する失敗応答が1件必要");
        };
        assert!(
            error.contains(&*path.to_string_lossy()) && error.contains("画像読み込み失敗"),
            "{error}"
        );
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
