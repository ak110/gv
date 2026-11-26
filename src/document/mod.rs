pub(crate) mod container;
pub(crate) mod types;
pub(crate) mod utils;

pub use types::{DocumentEvent, ErrorNotice, ZipBuffer};

const DECODE_FAILED_ACTION: &str =
    "ファイルが壊れていないか確認してください。他の画像へ移動すると表示を続けられます";
const PARTIAL_OPEN_ACTION: &str = "開けたコンテナの画像は表示を続けられます。開けなかったコンテナはファイルの破損やパスワード保護を確認してください";

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use anyhow::{Context as _, Result};
use crossbeam_channel::{Receiver, Sender};
use rayon::prelude::*;

use crate::archive::ArchiveManager;
use crate::extension_registry::ExtensionRegistry;
use crate::file_info::FileSource;
use crate::file_list::{FileList, NavigationDirection, SortOrder};
use crate::filter_spec::FilterKind;
use crate::image::{DecodedImage, DecoderChain};
use crate::persistent_filter::{FilterOperation, PersistentFilter};
use crate::prefetch::{PrefetchCoordinator, PrefetchEngine, PrefetchEvent};

use self::container::process_single_container;
use self::types::{ContainerExpandEvent, ContainerResult, ContainerState};
use self::utils::{build_expansion_pool, collect_folder_files_recursive, sort_paths_natural};

pub(crate) struct BookmarkSnapshot {
    pub entries: Vec<FileSource>,
    pub index: Option<usize>,
    pub expanded_pending: bool,
}

struct ActiveExpansion {
    rx: Receiver<ContainerExpandEvent>,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
struct ExpansionJob {
    generation: u64,
    active: Option<ActiveExpansion>,
}

impl ExpansionJob {
    fn start(&mut self, rx: Receiver<ContainerExpandEvent>) -> Arc<AtomicBool> {
        debug_assert!(self.active.is_none());
        let cancel = Arc::new(AtomicBool::new(false));
        self.active = Some(ActiveExpansion {
            rx,
            cancel: Arc::clone(&cancel),
        });
        cancel
    }

    fn cancel_and_advance(&mut self) {
        if let Some(active) = self.active.take() {
            active.cancel.store(true, Ordering::Relaxed);
        }
        self.generation += 1;
    }

    fn poll(&mut self) -> Option<(PathBuf, ContainerResult)> {
        loop {
            match self.active.as_ref()?.rx.try_recv() {
                Ok(ContainerExpandEvent::AllDone { generation }) => {
                    if generation == self.generation {
                        self.active = None;
                        return None;
                    }
                }
                Ok(ContainerExpandEvent::Expanded {
                    container_path,
                    result,
                    generation,
                }) => {
                    if generation == self.generation {
                        return Some((container_path, result));
                    }
                }
                Err(crossbeam_channel::TryRecvError::Empty) => return None,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    self.active = None;
                    return None;
                }
            }
        }
    }
}

#[cfg(test)]
struct ExpansionDispatch {
    paths: Vec<PathBuf>,
    tx: Sender<ContainerExpandEvent>,
    cancel: Arc<AtomicBool>,
    generation: u64,
}

pub struct Document {
    event_sender: Sender<DocumentEvent>,
    decoder: Arc<DecoderChain>,
    current_image: Option<DecodedImage>,
    file_list: FileList,
    prefetch_coord: PrefetchCoordinator,
    archive_manager: Arc<ArchiveManager>,
    archive_temp_dirs: Vec<types::ExtractedDirectory>,
    zip_buffers: Arc<RwLock<HashMap<PathBuf, ZipBuffer>>>,
    edited: bool,
    persistent_filter: PersistentFilter,
    expansion: ExpansionJob,
    container_states: HashMap<PathBuf, ContainerState>,
    last_navigation_direction: NavigationDirection,
    pending_navigation_intent: Option<(PathBuf, NavigationDirection)>,
    ui_notify: Option<Arc<dyn Fn() + Send + Sync>>,
    #[cfg(test)]
    expansion_dispatch: Option<Sender<ExpansionDispatch>>,
}

impl Document {
    pub fn new(
        event_sender: Sender<DocumentEvent>,
        decoder: Arc<DecoderChain>,
        registry: Arc<ExtensionRegistry>,
        archive_manager: ArchiveManager,
        default_sort: SortOrder,
    ) -> Self {
        let mut file_list = FileList::new(registry);
        file_list.set_sort_order(default_sort);
        Self {
            event_sender,
            decoder,
            current_image: None,
            file_list,
            prefetch_coord: PrefetchCoordinator::new(),
            archive_manager: Arc::new(archive_manager),
            archive_temp_dirs: Vec::new(),
            zip_buffers: Arc::new(RwLock::new(HashMap::new())),
            edited: false,
            persistent_filter: PersistentFilter::new(),
            expansion: ExpansionJob::default(),
            container_states: HashMap::new(),
            last_navigation_direction: NavigationDirection::Forward,
            pending_navigation_intent: None,
            ui_notify: None,
            #[cfg(test)]
            expansion_dispatch: None,
        }
    }

    pub fn start_prefetch(
        &mut self,
        notify: Arc<dyn Fn() + Send + Sync>,
        cache_budget: usize,
        base_image_size: usize,
    ) -> Result<()> {
        self.ui_notify = Some(Arc::clone(&notify));
        let engine = PrefetchEngine::new(
            Box::new(move || notify()),
            Arc::clone(&self.decoder),
            Arc::clone(&self.archive_manager),
            Arc::clone(&self.zip_buffers),
        )?;
        self.prefetch_coord
            .start(engine, cache_budget, base_image_size);
        Ok(())
    }

    pub fn process_prefetch_responses(&mut self) {
        let events = self
            .prefetch_coord
            .process_responses(self.file_list.current_index(), self.current_image.is_some());
        for event in events {
            match event {
                PrefetchEvent::CurrentImageReady(image) => {
                    self.current_image = Some(image);
                    let _ = self.event_sender.send(DocumentEvent::ImageReady);
                }
                PrefetchEvent::Error(error) => {
                    let _ = self.event_sender.send(DocumentEvent::Error(
                        ErrorNotice::new("画像を読み込めませんでした", DECODE_FAILED_ACTION)
                            .with_detail(error),
                    ));
                }
            }
        }
    }

    fn schedule_prefetch(&mut self) {
        let Some(center) = self.file_list.current_index() else {
            return;
        };
        self.prefetch_coord
            .reschedule(center, self.file_list.files(), &self.persistent_filter);
    }

    fn invalidate_cache(&mut self) {
        self.prefetch_coord.invalidate();
        self.file_list.clear_failed();
    }

    pub fn open(&mut self, path: &Path) -> Result<()> {
        let path = Self::canonicalize(path)?;

        if crate::bookmark::is_bookmark_file(&path) {
            return self.open_bookmark(&path);
        }

        if Self::is_pdf(&path) {
            return self.open_pdf(&path);
        }

        if self.archive_manager.is_archive(&path) {
            return self.open_archive(&path);
        }

        self.reset_document_state();

        if let Some(folder) = path.parent() {
            self.file_list.populate_from_folder(folder)?;
            self.file_list.set_current_by_path(&path);
            let _ = self.event_sender.send(DocumentEvent::FileListChanged);
        }

        self.load_current();
        Ok(())
    }

    pub fn open_single(&mut self, path: &Path) -> Result<()> {
        let path = Self::canonicalize(path)?;
        self.reset_document_state();
        self.file_list.populate_single(&path)?;
        let _ = self.event_sender.send(DocumentEvent::FileListChanged);
        self.load_current();
        Ok(())
    }

    fn open_bookmark(&mut self, bookmark_path: &Path) -> Result<()> {
        let is_archive = |p: &Path| self.archive_manager.is_archive(p);
        let data = crate::bookmark::load_bookmark_from_path(bookmark_path, &is_archive)?;
        self.load_bookmark_data(data)
    }

    fn open_archive(&mut self, archive_path: &Path) -> Result<()> {
        self.open_containers(&[archive_path.to_path_buf()])
    }

    fn open_pdf(&mut self, pdf_path: &Path) -> Result<()> {
        self.open_containers(&[pdf_path.to_path_buf()])
    }

    fn reset_document_state(&mut self) {
        self.expansion.cancel_and_advance();
        self.cleanup_archive_temp();
        self.invalidate_cache();
        self.file_list.clear();
        self.container_states.clear();
        self.pending_navigation_intent = None;
        self.current_image = None;
        self.edited = false;
    }

    pub fn open_containers(&mut self, paths: &[PathBuf]) -> Result<()> {
        self.reset_document_state();

        let errors = self.process_and_integrate_containers(paths);

        if self.file_list.len() == 0 {
            if errors.is_empty() {
                anyhow::bail!("コンテナ内に画像ファイルが存在しない");
            }
            anyhow::bail!("全てのコンテナの読み込みに失敗:\n{}", errors.join("\n"));
        }
        if !errors.is_empty() {
            let notice = ErrorNotice::new(
                format!("{}件のコンテナを開けませんでした", errors.len()),
                PARTIAL_OPEN_ACTION,
            )
            .with_detail(errors.join(" / "));
            let _ = self.event_sender.send(DocumentEvent::Error(notice));
        }

        self.file_list.sort_current();

        if self.file_list.len() > 0 {
            self.file_list.navigate_first();
        }
        let _ = self.event_sender.send(DocumentEvent::FileListChanged);

        self.start_background_expansion();

        self.load_current();
        Ok(())
    }

    pub fn open_multiple(&mut self, paths: &[PathBuf]) -> Result<()> {
        self.reset_document_state();

        let mut canonical_inputs: Vec<PathBuf> = Vec::new();
        for raw in paths {
            if let Ok(meta) = std::fs::symlink_metadata(raw)
                && utils::is_reparse_point(&meta)
            {
                continue;
            }
            let canon = Self::canonicalize(raw)?;
            if !canonical_inputs.contains(&canon) {
                canonical_inputs.push(canon);
            }
        }

        let mut filtered_inputs: Vec<PathBuf> = Vec::new();
        for path in &canonical_inputs {
            let is_descendant = canonical_inputs
                .iter()
                .any(|other| other != path && path.starts_with(other));
            if !is_descendant {
                filtered_inputs.push(path.clone());
            }
        }

        sort_paths_natural(&mut filtered_inputs);

        let mut toplevel_containers: Vec<PathBuf> = Vec::new();
        let mut folder_containers: Vec<PathBuf> = Vec::new();
        let mut bookmarks: Vec<PathBuf> = Vec::new();
        for path in &filtered_inputs {
            if path.is_dir() {
                for entry_path in collect_folder_files_recursive(path) {
                    if crate::bookmark::is_bookmark_file(&entry_path) {
                        bookmarks.push(entry_path);
                    } else if self.is_container(&entry_path) {
                        folder_containers.push(entry_path);
                    } else if let Some(name) = entry_path.file_name().and_then(|n| n.to_str())
                        && self.file_list.registry().is_image_extension(name)
                        && let Ok(info) = crate::file_info::FileInfo::from_path(&entry_path)
                    {
                        self.file_list.push(info);
                    }
                }
            } else if crate::bookmark::is_bookmark_file(path) {
                bookmarks.push(path.clone());
            } else if self.is_container(path) {
                toplevel_containers.push(path.clone());
            } else if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && self.file_list.registry().is_image_extension(name)
                && let Ok(info) = crate::file_info::FileInfo::from_path(path)
            {
                self.file_list.push(info);
            }
        }

        for bm_path in &bookmarks {
            let is_archive = |p: &Path| self.archive_manager.is_archive(p);
            if let Ok(data) = crate::bookmark::load_bookmark_from_path(bm_path, &is_archive) {
                for source in data.entries {
                    match source {
                        FileSource::File(path) => {
                            if path.exists()
                                && let Ok(info) = crate::file_info::FileInfo::from_path(&path)
                            {
                                self.file_list.push(info);
                            }
                        }
                        FileSource::ArchiveEntry { archive, .. }
                            if !toplevel_containers.contains(&archive)
                                && !folder_containers.contains(&archive) =>
                        {
                            toplevel_containers.push(archive);
                        }
                        FileSource::PendingContainer { container_path }
                            if !toplevel_containers.contains(&container_path)
                                && !folder_containers.contains(&container_path) =>
                        {
                            toplevel_containers.push(container_path);
                        }
                        _ => {}
                    }
                }
            }
        }

        sort_paths_natural(&mut toplevel_containers);
        let mut containers: Vec<PathBuf> = toplevel_containers;
        containers.extend(folder_containers);

        let errors = self.process_and_integrate_containers(&containers);

        if self.file_list.len() == 0 {
            if errors.is_empty() {
                anyhow::bail!("画像ファイルが存在しない");
            }
            anyhow::bail!("読み込みに失敗:\n{}", errors.join("\n"));
        }
        if !errors.is_empty() {
            let notice = ErrorNotice::new(
                format!("{}件のコンテナを開けませんでした", errors.len()),
                PARTIAL_OPEN_ACTION,
            )
            .with_detail(errors.join(" / "));
            let _ = self.event_sender.send(DocumentEvent::Error(notice));
        }

        self.file_list.sort_current();

        if self.file_list.len() > 0 {
            self.file_list.navigate_first();
        }
        let _ = self.event_sender.send(DocumentEvent::FileListChanged);

        self.start_background_expansion();

        self.load_current();
        Ok(())
    }

    fn process_and_integrate_containers(&mut self, paths: &[PathBuf]) -> Vec<String> {
        if paths.is_empty() {
            return Vec::new();
        }

        let mut errors: Vec<String> = Vec::new();

        let result = process_single_container(&paths[0], &self.archive_manager, Self::is_pdf);
        match result {
            ContainerResult::Error { path, error } => {
                errors.push(format!("{}: {error}", path.display()));
            }
            result => {
                self.integrate_container_result(result);
            }
        }

        for path in &paths[1..] {
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            let (file_size, modified) =
                std::fs::metadata(path).map_or((0, std::time::SystemTime::UNIX_EPOCH), |m| {
                    (
                        m.len(),
                        m.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                    )
                });
            let info = crate::file_info::FileInfo {
                source: FileSource::PendingContainer {
                    container_path: path.clone(),
                },
                file_name,
                file_size,
                modified,
                marked: false,
                load_failed: false,
            };
            self.file_list.push(info);
            self.container_states
                .insert(path.clone(), ContainerState::Pending);
        }

        errors
    }

    fn integrate_container_result(&mut self, result: ContainerResult) {
        let file_entries = Self::build_container_entries(&result);
        match result {
            ContainerResult::Error { .. } => {}
            ContainerResult::Pdf { path, page_count } => {
                if page_count == 0 {
                    return;
                }
                self.container_states
                    .insert(path.clone(), ContainerState::Expanded);
            }
            ContainerResult::Zip {
                path,
                buffer,
                entries,
            } => {
                if entries.is_empty() {
                    return;
                }
                self.zip_buffers
                    .write()
                    .expect("zip_buffers lock poisoned")
                    .insert(path.clone(), buffer);
                self.container_states
                    .insert(path.clone(), ContainerState::Expanded);
            }
            ContainerResult::TempExtracted {
                path,
                temp_dir,
                entries,
            } => {
                if entries.is_empty() {
                    return;
                }
                self.archive_temp_dirs.push(temp_dir);
                self.container_states
                    .insert(path.clone(), ContainerState::Expanded);
            }
        }
        for info in file_entries {
            self.file_list.push(info);
        }
    }

    fn build_container_entries(result: &ContainerResult) -> Vec<crate::file_info::FileInfo> {
        let mut entries = Vec::new();
        match result {
            ContainerResult::Error { .. } => {}
            ContainerResult::Pdf { path, page_count } => {
                let pdf_file_size = std::fs::metadata(path).map_or(0, |m| m.len());
                for i in 0..*page_count {
                    entries.push(crate::file_info::FileInfo {
                        source: FileSource::PdfPage {
                            pdf_path: path.clone(),
                            page_index: i,
                        },
                        file_name: format!("Page {:03}", i + 1),
                        file_size: pdf_file_size,
                        modified: std::fs::metadata(path)
                            .and_then(|metadata| metadata.modified())
                            .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                        marked: false,
                        load_failed: false,
                    });
                }
            }
            ContainerResult::Zip {
                path,
                entries: archive_entries,
                ..
            } => {
                for entry in archive_entries {
                    entries.push(crate::file_info::FileInfo {
                        source: FileSource::ArchiveEntry {
                            archive: path.clone(),
                            entry: entry.entry_name.clone(),
                            on_demand: true,
                            temp_path: None,
                            entry_index: Some(entry.entry_index),
                        },
                        file_name: entry.file_name.clone(),
                        file_size: entry.file_size,
                        modified: entry.modified,
                        marked: false,
                        load_failed: false,
                    });
                }
            }
            ContainerResult::TempExtracted {
                path,
                entries: temp_entries,
                ..
            } => {
                for (temp_path, entry_name, modified) in temp_entries {
                    if let Ok(mut info) = crate::file_info::FileInfo::from_path(temp_path) {
                        info.source = FileSource::ArchiveEntry {
                            archive: path.clone(),
                            entry: entry_name.clone(),
                            on_demand: false,
                            temp_path: Some(temp_path.clone()),
                            entry_index: None,
                        };
                        info.modified = *modified;
                        info.file_name = crate::archive::extract_filename(entry_name).to_string();
                        entries.push(info);
                    }
                }
            }
        }
        entries
    }

    fn start_background_expansion(&mut self) {
        let current_idx = self.file_list.current_index().unwrap_or(0);
        let mut pending_paths: Vec<(usize, PathBuf)> = Vec::new();
        for (i, file) in self.file_list.files().iter().enumerate() {
            if let FileSource::PendingContainer { container_path } = &file.source {
                if matches!(
                    self.container_states.get(container_path),
                    Some(ContainerState::Expanded)
                ) {
                    continue;
                }
                pending_paths.push((i, container_path.clone()));
            }
        }

        if pending_paths.is_empty() {
            return;
        }

        pending_paths.sort_by_key(|(idx, _)| (*idx as isize - current_idx as isize).unsigned_abs());

        let paths: Vec<PathBuf> = pending_paths.into_iter().map(|(_, p)| p).collect();

        for path in &paths {
            self.container_states
                .insert(path.clone(), ContainerState::InFlight);
        }

        let (tx, rx) = crossbeam_channel::unbounded();
        let cancel = self.expansion.start(rx);
        let generation = self.expansion.generation;
        #[cfg(test)]
        if let Some(dispatch) = &self.expansion_dispatch {
            assert!(
                dispatch
                    .send(ExpansionDispatch {
                        paths,
                        tx,
                        cancel,
                        generation
                    })
                    .is_ok()
            );
            return;
        }

        let archive_manager = Arc::clone(&self.archive_manager);
        let ui_notify = self.ui_notify.clone();
        let event_sender = self.event_sender.clone();

        std::thread::spawn(move || {
            let pool = match build_expansion_pool() {
                Ok(pool) => pool,
                Err(e) => {
                    let _ = event_sender.send(DocumentEvent::Error(
                        ErrorNotice::new(
                            "バックグラウンド展開用スレッドプールの作成に失敗しました",
                            "未展開のコンテナは表示した時点で展開します。改善しない場合はぐらびゅを再起動してください",
                        )
                        .with_detail(e.to_string()),
                    ));
                    let _ = tx.send(ContainerExpandEvent::AllDone { generation });
                    if let Some(notify) = &ui_notify {
                        notify();
                    }
                    return;
                }
            };

            pool.install(|| {
                paths.par_iter().for_each(|path| {
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let result = process_single_container(path, &archive_manager, Self::is_pdf);
                    let _ = tx.send(ContainerExpandEvent::Expanded {
                        container_path: path.clone(),
                        result,
                        generation,
                    });
                    if let Some(notify) = &ui_notify {
                        notify();
                    }
                });
            });

            let _ = tx.send(ContainerExpandEvent::AllDone { generation });
            if let Some(notify) = &ui_notify {
                notify();
            }
        });
    }

    fn reschedule_background_expansion(&mut self) {
        if !self.file_list.has_pending() {
            return;
        }
        self.expansion.cancel_and_advance();
        self.start_background_expansion();
    }

    pub fn process_expand_results(&mut self) {
        let mut applied = false;
        let mut intent_resolved = false;

        while let Some((container_path, result)) = self.expansion.poll() {
            let Some(idx) = self.find_pending_index(&container_path) else {
                continue;
            };

            let (direction, was_intent) = match &self.pending_navigation_intent {
                Some((p, d)) if *p == container_path => (*d, true),
                _ => (NavigationDirection::Forward, false),
            };

            let _ = self.apply_container_result(idx, &container_path, result, direction);
            applied = true;
            if was_intent {
                self.pending_navigation_intent = None;
                intent_resolved = true;
            }
        }

        if applied {
            self.invalidate_cache();
            if intent_resolved && self.file_list.current_index().is_some() {
                self.load_current();
            } else {
                self.schedule_prefetch();
            }
            let _ = self.event_sender.send(DocumentEvent::FileListChanged);
            self.send_navigation_changed();
        }
    }

    fn find_pending_index(&self, container_path: &Path) -> Option<usize> {
        self.file_list.files().iter().position(|f| match &f.source {
            FileSource::PendingContainer { container_path: cp } => cp == container_path,
            _ => false,
        })
    }

    pub fn expand_progress(&self) -> Option<(usize, usize)> {
        let total = self.container_states.len();
        if total == 0 {
            return None;
        }
        let done = self
            .container_states
            .values()
            .filter(|s| **s == ContainerState::Expanded)
            .count();
        if done >= total {
            None
        } else {
            Some((done, total))
        }
    }

    fn apply_container_result(
        &mut self,
        index: usize,
        container_path: &Path,
        result: ContainerResult,
        direction: NavigationDirection,
    ) -> Result<()> {
        match result {
            ContainerResult::Error {
                ref path,
                ref error,
            } => {
                let msg = format!("{}: {error}", path.display());
                self.file_list.remove_at(index);
                self.container_states
                    .insert(container_path.to_path_buf(), ContainerState::Expanded);
                let _ = self.event_sender.send(DocumentEvent::Error(
                    ErrorNotice::new(
                        "コンテナを展開できませんでした",
                        "このコンテナはリストから除き、他の画像の表示を続けています。ファイルの破損やパスワード保護を確認してください",
                    )
                    .with_detail(msg.clone()),
                ));
                anyhow::bail!("コンテナ展開失敗: {msg}");
            }
            result => {
                let entries = Self::build_container_entries(&result);

                if let ContainerResult::Zip { path, buffer, .. } = result {
                    self.zip_buffers
                        .write()
                        .expect("zip_buffers lock poisoned")
                        .insert(path, buffer);
                } else if let ContainerResult::TempExtracted { temp_dir, .. } = result
                    && !entries.is_empty()
                {
                    self.archive_temp_dirs.push(temp_dir);
                }

                self.file_list
                    .expand_container_at(index, entries, direction);
                self.container_states
                    .insert(container_path.to_path_buf(), ContainerState::Expanded);

                Ok(())
            }
        }
    }

    pub fn expand_all_pending_sync(&mut self) {
        self.expansion.cancel_and_advance();

        while self.file_list.has_pending() {
            let pending = self
                .file_list
                .files()
                .iter()
                .enumerate()
                .find(|(_, f)| f.source.is_pending_container());
            let Some((idx, file)) = pending else { break };
            let container_path = match &file.source {
                FileSource::PendingContainer { container_path } => container_path.clone(),
                _ => break,
            };
            if matches!(
                self.container_states.get(&container_path),
                Some(ContainerState::Expanded)
            ) {
                self.file_list.remove_at(idx);
                continue;
            }
            self.container_states
                .insert(container_path.clone(), ContainerState::InFlight);
            let result =
                process_single_container(&container_path, &self.archive_manager, Self::is_pdf);
            let _ = self.apply_container_result(
                idx,
                &container_path,
                result,
                NavigationDirection::Forward,
            );
        }

        self.file_list.clear_failed();
        if let Some(center) = self.file_list.current_index() {
            self.prefetch_coord.invalidate_and_reschedule(
                center,
                self.file_list.files(),
                &self.persistent_filter,
            );
        } else {
            self.prefetch_coord.invalidate();
        }
        let _ = self.event_sender.send(DocumentEvent::FileListChanged);
    }

    pub(crate) fn bookmark_snapshot(&mut self) -> BookmarkSnapshot {
        let expanded_pending = self.file_list.has_pending();
        if expanded_pending {
            self.expand_all_pending_sync();
        }
        BookmarkSnapshot {
            entries: self
                .file_list
                .files()
                .iter()
                .map(|file| file.source.clone())
                .collect(),
            index: self.file_list.current_index(),
            expanded_pending,
        }
    }

    fn is_pdf(path: &Path) -> bool {
        ExtensionRegistry::is_pdf_path(path)
    }

    fn cleanup_archive_temp(&mut self) {
        self.archive_temp_dirs.clear();
        self.zip_buffers
            .write()
            .expect("zip_buffers lock poisoned")
            .clear();
    }

    pub fn open_folder(&mut self, folder: &Path) -> Result<()> {
        std::fs::read_dir(folder)?;
        self.open_multiple(&[folder.to_path_buf()])
    }

    pub fn navigate_relative(&mut self, offset: isize) {
        self.last_navigation_direction = if offset >= 0 {
            NavigationDirection::Forward
        } else {
            NavigationDirection::Backward
        };
        if self.file_list.navigate_relative(offset) {
            self.load_current();
        }
    }

    pub fn navigate_first(&mut self) {
        self.last_navigation_direction = NavigationDirection::Forward;
        if self.file_list.navigate_first() {
            self.load_current();
        }
    }

    pub fn navigate_to(&mut self, index: usize) {
        self.last_navigation_direction = NavigationDirection::Forward;
        if self.file_list.navigate_to(index) {
            self.load_current();
        }
    }

    pub fn navigate_last(&mut self) {
        self.last_navigation_direction = NavigationDirection::Backward;
        if self.file_list.navigate_last() {
            self.load_current();
        }
    }

    fn load_current(&mut self) {
        self.edited = false;
        let Some(index) = self.file_list.current_index() else {
            self.current_image = None;
            return;
        };

        if self.file_list.is_pending_at(index)
            && let Some(container_path) = self.file_list.pending_container_path_at(index)
        {
            self.current_image = None;

            self.pending_navigation_intent = Some((container_path, self.last_navigation_direction));
            self.reschedule_background_expansion();

            self.send_navigation_changed();
            let _ = self.event_sender.send(DocumentEvent::ImageReady);
            return;
        }

        self.pending_navigation_intent = None;

        if let Some(image) = self.prefetch_coord.take(index) {
            self.current_image = Some(image);
            let _ = self.event_sender.send(DocumentEvent::ImageReady);
            self.send_navigation_changed();
            self.schedule_prefetch();
            return;
        }

        let current = self.file_list.current().expect("current_index was Some");
        let source = current.source.clone();

        let decode_result = crate::image::decode_source(
            &source,
            &self.decoder,
            &self.archive_manager,
            &self.zip_buffers,
            true,
        );

        match decode_result {
            Ok(image) => {
                let image = self.persistent_filter.apply(&image).unwrap_or(image);
                self.current_image = Some(image);
                let _ = self.event_sender.send(DocumentEvent::ImageReady);
            }
            Err(e) => {
                self.current_image = None;
                self.file_list.mark_failed(index);
                let msg = format!("{}: {}", source.display_path(), e);
                let _ = self.event_sender.send(DocumentEvent::Error(
                    ErrorNotice::new("画像を読み込めませんでした", DECODE_FAILED_ACTION)
                        .with_detail(msg),
                ));
            }
        }

        self.send_navigation_changed();
        self.schedule_prefetch();
    }

    fn send_navigation_changed(&self) {
        if let Some(index) = self.file_list.current_index() {
            let _ = self
                .event_sender
                .send(DocumentEvent::NavigationChanged { index });
        }
    }

    fn canonicalize(path: &Path) -> Result<PathBuf> {
        match std::fs::canonicalize(path) {
            Ok(canonical) => Ok(crate::util::strip_extended_length_prefix(&canonical)),
            Err(e) => {
                if path.exists() {
                    Ok(path.to_path_buf())
                } else {
                    Err(e).with_context(|| format!("パス解決失敗: {}", path.display()))
                }
            }
        }
    }

    fn read_file_data(&self, info: &crate::file_info::FileInfo) -> Result<Vec<u8>> {
        info.source
            .read_bytes(&self.archive_manager, &self.zip_buffers)
    }

    #[cfg(test)]
    pub fn read_file_data_current(&self) -> Result<Vec<u8>> {
        let current = self
            .file_list
            .current()
            .ok_or_else(|| anyhow::anyhow!("ファイルが選択されていない"))?;
        self.read_file_data(current)
    }

    pub fn copy_source_to(&self, source: &FileSource, destination: &Path) -> Result<()> {
        match source {
            FileSource::File(path)
            | FileSource::ArchiveEntry {
                on_demand: false,
                temp_path: Some(path),
                ..
            } => crate::shell::file_operations::copy_atomic(path, destination),
            _ => crate::shell::file_operations::write_atomic(
                destination,
                &source.read_bytes(&self.archive_manager, &self.zip_buffers)?,
            ),
        }
    }

    pub fn current_image(&self) -> Option<&DecodedImage> {
        self.current_image.as_ref()
    }

    pub fn current_path(&self) -> Option<&Path> {
        self.file_list.current().and_then(|f| f.source.file_path())
    }

    pub fn file_list(&self) -> &FileList {
        &self.file_list
    }

    #[cfg(test)]
    pub(crate) fn append_test_file(&mut self, info: crate::file_info::FileInfo) {
        self.file_list.push(info);
    }

    pub fn is_container(&self, path: &Path) -> bool {
        self.archive_manager.is_archive(path)
            || Self::is_pdf(path)
            || crate::bookmark::is_bookmark_file(path)
    }

    pub fn is_archive_path(&self, path: &Path) -> bool {
        self.archive_manager.is_archive(path)
    }

    pub fn current_source(&self) -> Option<&FileSource> {
        self.file_list.current().map(|f| &f.source)
    }

    pub fn mark_current(&mut self) {
        if let Some(index) = self.file_list.current_index() {
            self.file_list.mark_at(index);
        }
    }

    pub fn unmark_current(&mut self) {
        if let Some(index) = self.file_list.current_index() {
            self.file_list.unmark_at(index);
        }
    }

    pub fn invert_all_marks(&mut self) {
        self.file_list.invert_all_marks();
    }

    pub fn invert_marks_to_here(&mut self) {
        self.file_list.invert_marks_to_here();
    }

    pub fn navigate_prev_mark(&mut self) {
        self.last_navigation_direction = NavigationDirection::Backward;
        if self.file_list.navigate_prev_mark() {
            self.load_current();
        }
    }

    pub fn navigate_next_mark(&mut self) {
        self.last_navigation_direction = NavigationDirection::Forward;
        if self.file_list.navigate_next_mark() {
            self.load_current();
        }
    }

    pub fn navigate_prev_folder(&mut self) {
        self.last_navigation_direction = NavigationDirection::Forward;
        if self.file_list.navigate_prev_folder() {
            self.load_current();
        }
    }

    pub fn navigate_next_folder(&mut self) {
        self.last_navigation_direction = NavigationDirection::Forward;
        if self.file_list.navigate_next_folder() {
            self.load_current();
        }
    }

    pub fn sort_navigate_back(&mut self) {
        self.last_navigation_direction = NavigationDirection::Backward;
        let order = self.file_list.sort_order();
        if self.file_list.sorted_navigate(-1, order) {
            self.load_current();
        }
    }

    pub fn shuffle_all(&mut self) {
        self.file_list.shuffle_all();
        self.after_list_change();
    }

    pub fn shuffle_groups(&mut self) {
        self.file_list.shuffle_groups();
        self.after_list_change();
    }

    pub fn sort_navigate_forward(&mut self) {
        self.last_navigation_direction = NavigationDirection::Forward;
        let order = self.file_list.sort_order();
        if self.file_list.sorted_navigate(1, order) {
            self.load_current();
        }
    }

    pub fn remove_current_from_list(&mut self) {
        if let Some(index) = self.file_list.current_index() {
            self.file_list.remove_at(index);
            self.after_list_change();
        }
    }

    pub fn remove_indices_from_list(&mut self, indices: &[usize]) {
        for &index in indices.iter().rev() {
            self.file_list.remove_at(index);
        }
        self.after_list_change();
    }

    pub fn remove_marked_from_list(&mut self) {
        if self.file_list.marked_count() == 0 {
            return;
        }
        self.file_list.remove_marked();
        self.after_list_change();
    }

    pub fn after_list_change(&mut self) {
        self.invalidate_cache();
        self.reschedule_background_expansion();
        let _ = self.event_sender.send(DocumentEvent::FileListChanged);
        if self.file_list.len() > 0 {
            self.load_current();
        } else {
            self.current_image = None;
            self.edited = false;
            let _ = self.event_sender.send(DocumentEvent::ImageReady);
        }
    }

    pub fn reload(&mut self) {
        self.invalidate_cache();
        self.load_current();
    }

    pub fn close_all(&mut self) {
        self.reset_document_state();
        let _ = self.event_sender.send(DocumentEvent::FileListChanged);
        let _ = self.event_sender.send(DocumentEvent::ImageReady);
    }

    pub fn load_bookmark_data(&mut self, data: crate::bookmark::BookmarkData) -> Result<()> {
        let has_containers = data.entries.iter().any(|s| {
            matches!(
                s,
                FileSource::PdfPage { .. }
                    | FileSource::ArchiveEntry { .. }
                    | FileSource::PendingContainer { .. }
            )
        });

        if has_containers {
            let mut container_paths: Vec<PathBuf> = Vec::new();
            for source in &data.entries {
                let path = match source {
                    FileSource::PdfPage { pdf_path, .. } => Some(pdf_path),
                    FileSource::ArchiveEntry { archive, .. } => Some(archive),
                    FileSource::PendingContainer { container_path } => Some(container_path),
                    FileSource::File(_) => None,
                };
                if let Some(p) = path
                    && p.exists()
                    && !container_paths.contains(p)
                {
                    container_paths.push(p.clone());
                }
            }

            if container_paths.is_empty() {
                anyhow::bail!("ブックマーク内のコンテナが見つからない");
            }

            self.open_containers(&container_paths)?;

            self.expand_all_pending_sync();

            if let Some(target_source) = data.entries.get(data.index) {
                let target_idx = self
                    .file_list
                    .files()
                    .iter()
                    .position(|f| FileList::source_matches(&f.source, target_source))
                    .unwrap_or_else(|| data.index.min(self.file_list.len().saturating_sub(1)));
                self.file_list.navigate_to(target_idx);
                self.load_current();
            }
        } else {
            self.reset_document_state();

            for source in &data.entries {
                if let FileSource::File(path) = source
                    && path.exists()
                    && let Ok(info) = crate::file_info::FileInfo::from_path(path)
                {
                    self.file_list.push(info);
                }
            }

            if self.file_list.len() > 0 {
                let idx = data.index.min(self.file_list.len() - 1);
                self.file_list.navigate_to(idx);
            }

            let _ = self.event_sender.send(DocumentEvent::FileListChanged);
            self.load_current();
        }

        Ok(())
    }

    pub fn current_metadata(&self) -> Result<crate::image::ImageMetadata> {
        let current = self
            .file_list
            .current()
            .ok_or_else(|| anyhow::anyhow!("ファイルが選択されていない"))?;

        if matches!(current.source, FileSource::PdfPage { .. }) {
            return if self.current_image.is_some() {
                Ok(crate::image::ImageMetadata {
                    format: "PDF".to_string(),
                    comments: Vec::new(),
                    exif: Vec::new(),
                })
            } else {
                anyhow::bail!("PDFページが未レンダリング")
            };
        }

        let data = self.read_file_data(current)?;
        self.decoder
            .metadata(&data, &current.source.filename_hint())
    }

    pub fn is_cached(&self, index: usize) -> bool {
        self.prefetch_coord.contains(index)
            || (self.file_list.current_index() == Some(index) && self.current_image.is_some())
    }

    pub fn has_unsaved_edit(&self) -> bool {
        self.edited
    }

    pub fn persistent_filter(&self) -> &PersistentFilter {
        &self.persistent_filter
    }

    pub fn add_persistent_filter(&mut self, operation: FilterOperation) {
        self.persistent_filter.add_operation(operation);
        self.on_persistent_filter_changed();
    }

    pub fn remove_persistent_filter(&mut self, kind: FilterKind) -> bool {
        if !self.persistent_filter.remove_operation_type(kind) {
            return false;
        }
        self.on_persistent_filter_changed();
        true
    }

    pub fn toggle_persistent_filter(&mut self) {
        self.persistent_filter.toggle_enabled();
        self.on_persistent_filter_changed();
    }

    fn on_persistent_filter_changed(&mut self) {
        self.invalidate_cache();
        self.load_current();
    }

    pub fn apply_edit(&mut self, new_image: DecodedImage) {
        self.current_image = Some(new_image);
        self.edited = true;
        let _ = self.event_sender.send(DocumentEvent::ImageReady);
    }
}

impl Drop for Document {
    fn drop(&mut self) {
        self.expansion.cancel_and_advance();
        self.cleanup_archive_temp();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{create_1x1_white_png, setup_test_dir};
    use std::io::Write;

    use crate::test_helpers::test_document;

    #[test]
    fn background_expansion_operations_discard_old_results_and_resolve_navigation() {
        let (mut doc, _events) = test_document();
        let root = setup_test_dir("expansion_sequence", 0);
        let path = PathBuf::from("current.zip");
        doc.file_list
            .push(crate::test_helpers::make_pending_container_info(
                "current.zip",
            ));
        let (dispatch, worker) = crossbeam_channel::unbounded();
        doc.expansion_dispatch = Some(dispatch);

        let stale_result = |name: &str, generation| {
            let temp_dir = root.join(name);
            std::fs::create_dir(&temp_dir).unwrap();
            let event = ContainerExpandEvent::Expanded {
                container_path: path.clone(),
                result: ContainerResult::TempExtracted {
                    path: path.clone(),
                    temp_dir: types::ExtractedDirectory(temp_dir.clone()),
                    entries: Vec::new(),
                },
                generation,
            };
            (event, temp_dir)
        };

        let phase = |doc: &Document| (doc.expansion.generation, doc.expansion.active.is_some());
        let legal_edges = [
            ((0, false), (0, true)),
            ((0, true), (1, true)),
            ((1, true), (1, false)),
        ];
        let mut before = phase(&doc);
        let mut check = |doc: &Document, expected| {
            let after = phase(doc);
            assert_eq!(after, expected);
            assert!(before == after || legal_edges.contains(&(before, after)));
            before = after;
        };

        doc.start_background_expansion();
        check(&doc, (0, true));
        let old = worker.try_recv().unwrap();
        assert_eq!(old.paths, std::slice::from_ref(&path));
        assert_eq!(old.generation, 0);
        let (queued, queued_dir) = stale_result("queued", 0);
        old.tx.send(queued).unwrap();

        doc.navigate_last();
        check(&doc, (1, true));
        assert_eq!(
            doc.pending_navigation_intent,
            Some((path.clone(), NavigationDirection::Backward))
        );
        assert!(old.cancel.load(Ordering::Relaxed));
        assert!(!queued_dir.exists());
        let (late, late_dir) = stale_result("late", 0);
        assert!(old.tx.send(late).is_err());
        assert!(!late_dir.exists());

        let ExpansionDispatch {
            paths,
            tx,
            cancel,
            generation,
        } = worker.try_recv().unwrap();
        assert_eq!(paths, std::slice::from_ref(&path));
        assert_eq!(generation, 1);
        assert!(!Arc::ptr_eq(&cancel, &old.cancel));
        let (stale, stale_dir) = stale_result("stale", 0);
        tx.send(ContainerExpandEvent::AllDone { generation: 0 })
            .unwrap();
        tx.send(stale).unwrap();
        doc.process_expand_results();
        check(&doc, (1, true));
        assert!(!stale_dir.exists());
        assert!(doc.archive_temp_dirs.is_empty());
        assert_eq!(doc.find_pending_index(&path), Some(0));
        assert!(doc.pending_navigation_intent.is_some());

        let current_dir = root.join("current");
        std::fs::create_dir(&current_dir).unwrap();
        let entries = ["first.png", "last.png"].map(|name| {
            let file = current_dir.join(name);
            std::fs::write(&file, create_1x1_white_png()).unwrap();
            (file, name.into(), std::time::SystemTime::UNIX_EPOCH)
        });
        tx.send(ContainerExpandEvent::Expanded {
            container_path: path.clone(),
            result: ContainerResult::TempExtracted {
                path: path.clone(),
                temp_dir: types::ExtractedDirectory(current_dir.clone()),
                entries: entries.into(),
            },
            generation: 1,
        })
        .unwrap();
        doc.process_expand_results();
        check(&doc, (1, true));
        assert!(doc.pending_navigation_intent.is_none());
        assert!(!doc.file_list.has_pending());
        assert_eq!(doc.file_list.files().len(), 2);
        assert_eq!(doc.file_list.current_index(), Some(1));
        assert!(doc.current_image.is_some());
        assert!(current_dir.is_dir());

        tx.send(ContainerExpandEvent::AllDone { generation: 1 })
            .unwrap();
        doc.process_expand_results();
        check(&doc, (1, false));
        assert!(!cancel.load(Ordering::Relaxed));
        assert_eq!(Arc::strong_count(&cancel), 1);
    }

    #[test]
    fn new_initial_state() {
        let (doc, _rx) = test_document();
        assert!(doc.current_image().is_none());
        assert!(doc.current_path().is_none());
        assert_eq!(doc.file_list().len(), 0);
        assert_eq!(doc.file_list().current_index(), None);
    }

    #[test]
    fn new_propagates_default_sort_to_file_list() {
        let (doc, _rx) = test_document();
        assert_eq!(doc.file_list().sort_order(), SortOrder::default());

        let (sender, _rx2) = crossbeam_channel::unbounded();
        let registry = Arc::new(ExtensionRegistry::new());
        let decoder = crate::test_helpers::test_decoder();
        let archive_manager = crate::test_helpers::test_archive_manager(&registry);
        let mut doc2 = Document::new(sender, decoder, registry, archive_manager, SortOrder::Date);
        let dir = setup_test_dir("date_sort", 2);
        for (name, seconds) in [
            ("image_000.png", 1_600_000_100),
            ("image_001.png", 1_600_000_000),
        ] {
            std::fs::OpenOptions::new()
                .write(true)
                .open(dir.join(name))
                .unwrap()
                .set_modified(
                    std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(seconds),
                )
                .unwrap();
        }
        doc2.open_folder(&dir).unwrap();
        let names: Vec<_> = doc2
            .file_list()
            .files()
            .iter()
            .map(|file| file.file_name.as_str())
            .collect();
        assert_eq!(names, ["image_001.png", "image_000.png"]);
        assert_eq!(
            doc2.file_list().current().unwrap().file_name,
            "image_001.png"
        );
    }

    #[test]
    fn open_folder_populates_list() {
        let dir = setup_test_dir("open", 3);
        let (mut doc, _rx) = test_document();
        doc.open_folder(&dir).unwrap();

        let names: Vec<_> = doc
            .file_list()
            .files()
            .iter()
            .map(|file| file.file_name.as_str())
            .collect();
        assert_eq!(names, ["image_000.png", "image_001.png", "image_002.png"]);
        assert_eq!(doc.current_image().unwrap().data, [255; 4]);
        assert_eq!(doc.file_list().current_index(), Some(0));
    }

    fn write_test_pixel(path: &Path, rgba: [u8; 4]) {
        image::RgbaImage::from_pixel(1, 1, image::Rgba(rgba))
            .save(path)
            .unwrap();
    }

    #[test]
    fn navigate_relative_forward_backward() {
        let dir = setup_test_dir("navigate", 5);
        write_test_pixel(&dir.join("image_002.png"), [20, 255, 255, 255]);
        write_test_pixel(&dir.join("image_001.png"), [10, 255, 255, 255]);
        let (mut doc, events) = test_document();
        doc.open_folder(&dir).unwrap();
        events.try_iter().for_each(drop);

        assert_eq!(doc.file_list().current_index(), Some(0));
        doc.navigate_relative(2);
        assert_eq!(doc.file_list().current_index(), Some(2));
        assert!(
            events
                .try_iter()
                .any(|event| matches!(event, DocumentEvent::NavigationChanged { index: 2 }))
        );
        assert_eq!(doc.current_image().unwrap().data, [20, 255, 255, 255]);
        doc.navigate_relative(-1);
        assert_eq!(doc.file_list().current_index(), Some(1));
        assert!(
            events
                .try_iter()
                .any(|event| matches!(event, DocumentEvent::NavigationChanged { index: 1 }))
        );
        assert_eq!(doc.current_image().unwrap().data, [10, 255, 255, 255]);
    }

    #[test]
    fn navigate_first_last() {
        let dir = setup_test_dir("first_last", 5);
        write_test_pixel(&dir.join("image_004.png"), [40, 255, 255, 255]);
        let (mut doc, _rx) = test_document();
        doc.open_folder(&dir).unwrap();

        doc.navigate_last();
        assert_eq!(doc.file_list().current_index(), Some(4));
        assert_eq!(doc.current_image().unwrap().data, [40, 255, 255, 255]);
        doc.navigate_first();
        assert_eq!(doc.file_list().current_index(), Some(0));
        assert_eq!(doc.current_image().unwrap().data, [255, 255, 255, 255]);
    }

    #[test]
    fn navigate_to_index() {
        let dir = setup_test_dir("to_index", 5);
        write_test_pixel(&dir.join("image_003.png"), [30, 255, 255, 255]);
        let (mut doc, events) = test_document();
        doc.open_folder(&dir).unwrap();
        events.try_iter().for_each(drop);

        doc.navigate_to(3);
        assert_eq!(doc.file_list().current_index(), Some(3));
        assert!(
            events
                .try_iter()
                .any(|event| matches!(event, DocumentEvent::NavigationChanged { index: 3 }))
        );
        assert_eq!(doc.current_image().unwrap().data, [30, 255, 255, 255]);
    }

    #[test]
    fn mark_operations() {
        let dir = setup_test_dir("marks", 3);
        let (mut doc, _rx) = test_document();
        doc.open_folder(&dir).unwrap();

        assert_eq!(doc.file_list().marked_count(), 0);

        doc.mark_current();
        assert_eq!(doc.file_list().current_index(), Some(0));
        assert_eq!(doc.file_list().marked_indices(), [0]);

        doc.invert_all_marks();
        assert_eq!(doc.file_list().marked_indices(), [1, 2]);
    }

    #[test]
    fn is_pdf_detection() {
        let dir = setup_test_dir("pdf_detection", 0);
        for name in ["first.pdf", "second.PDF"] {
            let path = dir.join(name);
            std::fs::write(&path, crate::test_helpers::minimal_pdf()).unwrap();
            let (mut doc, _rx) = test_document();
            doc.open(&path).unwrap();
            assert!(matches!(
                &doc.file_list().current().unwrap().source,
                FileSource::PdfPage { pdf_path, page_index: 0 } if pdf_path.ends_with(name)
            ));
            assert!(doc.current_image().is_some());
        }
        assert!(!Document::is_pdf(Path::new("test.png")));
        assert!(!Document::is_pdf(Path::new("test.pdf.txt")));
    }

    #[test]
    fn orientation_is_applied_consistently_for_file_archive_and_prefetch() {
        use crate::test_helpers::{
            asymmetric_rgba, encode_with_exif, exif_with_orientation, expected_oriented,
        };
        let dir = crate::test_helpers::TempDir::new("document_orientation");
        std::fs::create_dir_all(&dir).unwrap();
        let src = asymmetric_rgba(3, 2);
        let orientations = [6u16, 5];
        let files: Vec<(String, Vec<u8>)> = orientations
            .iter()
            .enumerate()
            .map(|(i, &o)| {
                (
                    format!("image_{i}.png"),
                    encode_with_exif(
                        &src,
                        image::ImageFormat::Png,
                        Some(exif_with_orientation(o)),
                    ),
                )
            })
            .collect();
        let folder = dir.join("folder");
        std::fs::create_dir_all(&folder).unwrap();
        for (name, data) in &files {
            std::fs::write(folder.join(name), data).unwrap();
        }
        let zip_path = dir.join("images.zip");
        {
            let mut writer = ::zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let opts = ::zip::write::SimpleFileOptions::default()
                .compression_method(::zip::CompressionMethod::Stored);
            for (name, data) in &files {
                writer.start_file(name.as_str(), opts).unwrap();
                writer.write_all(data).unwrap();
            }
            writer.finish().unwrap();
        }
        let expected: Vec<Vec<u8>> = orientations
            .iter()
            .map(|&o| expected_oriented(&src, o).into_raw())
            .collect();
        let current = |doc: &Document| {
            let img = doc.current_image().expect("image loaded");
            assert_eq!((img.width, img.height), (2, 3));
            img.data.clone()
        };

        let (mut doc, _rx) = test_document();
        doc.start_prefetch(Arc::new(|| {}), 64 * 1024 * 1024, 1024)
            .unwrap();
        doc.open_folder(&folder).unwrap();
        assert_eq!(current(&doc), expected[0], "file");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !doc.prefetch_coord.contains(1) {
            assert!(std::time::Instant::now() < deadline, "prefetch timed out");
            doc.process_prefetch_responses();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        doc.navigate_relative(1);
        assert_eq!(current(&doc), expected[1], "prefetched");
        doc.navigate_relative(-1);
        assert_eq!(current(&doc), expected[0], "back");
        doc.reload();
        assert_eq!(current(&doc), expected[0], "reload");

        let (mut zip_doc, _rx) = test_document();
        zip_doc.open(&zip_path).unwrap();
        assert_eq!(current(&zip_doc), expected[0], "archive");
        zip_doc.navigate_relative(1);
        assert_eq!(current(&zip_doc), expected[1], "archive next");

        drop(doc);
        drop(zip_doc);
    }
    #[test]
    fn new_list_entrances_cancel_old_expansion_and_edit() {
        let dir = setup_test_dir("reset_entrances", 2);
        let image = dir.join("image_000.png");
        let archive = dir.join("images.zip");
        let mut writer = ::zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        writer
            .start_file("inside.png", ::zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(&create_1x1_white_png()).unwrap();
        writer.finish().unwrap();
        for entrance in 0..7 {
            let (mut doc, _rx) = test_document();
            doc.open_single(&image).unwrap();
            doc.apply_edit(crate::image::DecodedImage {
                data: vec![0, 0, 0, 255],
                width: 1,
                height: 1,
            });
            let old = dir.join("old.zip");
            let other_old = dir.join("other_old.zip");
            let (sender, receiver) = crossbeam_channel::unbounded();
            let flag = doc.expansion.start(receiver);
            doc.container_states
                .insert(old.clone(), ContainerState::InFlight);
            doc.container_states
                .insert(other_old.clone(), ContainerState::InFlight);
            doc.pending_navigation_intent = Some((old.clone(), NavigationDirection::Forward));
            let generation = doc.expansion.generation;
            assert_eq!(doc.expand_progress(), Some((0, 2)));
            match entrance {
                0 => doc.open(&image).unwrap(),
                1 => doc.open_single(&image).unwrap(),
                2 => doc.close_all(),
                3 => doc
                    .load_bookmark_data(crate::bookmark::BookmarkData {
                        entries: vec![FileSource::File(image.clone())],
                        index: 0,
                    })
                    .unwrap(),
                4 => doc
                    .open_multiple(&[image.clone(), dir.join("image_001.png")])
                    .unwrap(),
                5 => doc.open_containers(std::slice::from_ref(&archive)).unwrap(),
                _ => doc.open_folder(&dir).unwrap(),
            }
            assert!(flag.load(Ordering::Relaxed));
            assert!(doc.expansion.generation > generation);
            assert!(!doc.container_states.contains_key(&old));
            assert!(!doc.container_states.contains_key(&other_old));
            if entrance <= 3 {
                assert!(doc.expand_progress().is_none());
                assert!(doc.expansion.active.is_none());
            }
            assert!(doc.pending_navigation_intent.is_none());
            assert!(!doc.has_unsaved_edit());
            assert!(
                sender
                    .send(ContainerExpandEvent::AllDone { generation })
                    .is_err()
            );
        }
    }

    #[test]
    fn successful_move_removes_only_target_rows_and_marks() {
        let dir = setup_test_dir("moved_rows", 3);
        let (mut doc, _rx) = test_document();
        doc.open_folder(&dir).unwrap();
        doc.file_list.mark_at(0);
        doc.file_list.mark_at(2);
        std::fs::rename(dir.join("image_000.png"), dir.join("moved.png")).unwrap();
        doc.remove_indices_from_list(&[0]);
        assert_eq!(doc.file_list().len(), 2);
        assert_eq!(doc.file_list().marked_indices(), [1]);
        assert_eq!(doc.file_list().files()[0].file_name, "image_001.png");
        assert_eq!(doc.file_list().files()[1].file_name, "image_002.png");
        doc.navigate_to(1);
        std::fs::rename(dir.join("image_002.png"), dir.join("moved2.png")).unwrap();
        doc.remove_current_from_list();
        assert_eq!(doc.file_list().len(), 1);
        assert!(doc.file_list().marked_indices().is_empty());
        assert_eq!(doc.file_list().files()[0].file_name, "image_001.png");
    }

    #[test]
    fn container_metadata_uses_original_timestamps() {
        let dir = setup_test_dir("metadata", 1);
        let path = dir.join("original.pdf");
        std::fs::write(&path, crate::test_helpers::minimal_pdf()).unwrap();
        let expected =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(expected)
            .unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            expected,
            "fixture"
        );
        let (mut doc, _rx) = test_document();
        doc.open(&path).unwrap();
        assert_eq!(doc.file_list().current().unwrap().modified, expected);
        let image = dir.join("image_000.png");
        let extracted = Document::build_container_entries(&ContainerResult::TempExtracted {
            path: path.clone(),
            temp_dir: types::ExtractedDirectory(dir.join("extracted")),
            entries: vec![(image, "inside.png".into(), expected)],
        });
        assert_eq!(extracted[0].modified, expected);
    }

    #[test]
    fn zip_header_timestamp_reaches_list_metadata() {
        let dir = setup_test_dir("zip_header_timestamp", 0);
        let path = dir.join("timestamp.zip");
        let time = ::zip::DateTime::from_date_and_time(2020, 9, 13, 12, 26, 40).unwrap();
        let mut writer = ::zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        writer
            .start_file(
                "inside.png",
                ::zip::write::SimpleFileOptions::default().last_modified_time(time),
            )
            .unwrap();
        writer.write_all(&create_1x1_white_png()).unwrap();
        writer.finish().unwrap();
        let expected =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
        let registry = Arc::new(ExtensionRegistry::new());
        let manager = ArchiveManager::new(registry);
        let entries = manager
            .list_images_from_buffer(&std::fs::read(&path).unwrap(), &path)
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].modified, expected);
        let (mut doc, _rx) = test_document();
        doc.open(&path).unwrap();
        assert!(doc.current_image().is_some());
        assert_eq!(doc.file_list().len(), 1);
        assert_eq!(doc.file_list().current().unwrap().modified, expected);
    }

    #[test]
    fn changed_filter_discards_old_worker_result_before_displaying_new_generation() {
        use crate::persistent_filter::FilterOperation;
        use std::sync::atomic::AtomicUsize;
        use std::time::Duration;

        struct GatedDecoder {
            started: Sender<usize>,
            release: Receiver<()>,
            worker_calls: AtomicUsize,
        }
        impl crate::image::ImageDecoder for GatedDecoder {
            fn can_decode(&self, _data: &[u8], _hint: &str) -> bool {
                true
            }
            fn decode(&self, _data: &[u8], _hint: &str) -> Result<DecodedImage> {
                if std::thread::current().name() == Some("prefetch-worker") {
                    let call = self.worker_calls.fetch_add(1, Ordering::Relaxed);
                    self.started.send(call)?;
                    self.release.recv_timeout(Duration::from_secs(5))?;
                }
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

        let dir = setup_test_dir("filtered_worker_generation", 2);
        let (started_tx, started_rx) = crossbeam_channel::unbounded();
        let (release_tx, release_rx) = crossbeam_channel::unbounded();
        let (notified_tx, notified_rx) = crossbeam_channel::unbounded();
        let (event_tx, event_rx) = crossbeam_channel::unbounded();
        let decoder = Arc::new(DecoderChain::new(vec![Box::new(GatedDecoder {
            started: started_tx,
            release: release_rx,
            worker_calls: AtomicUsize::new(0),
        })]));
        let registry = Arc::new(ExtensionRegistry::new());
        let manager = ArchiveManager::new(Arc::clone(&registry));
        let mut doc = Document::new(event_tx, decoder, registry, manager, SortOrder::default());
        doc.start_prefetch(
            Arc::new(move || {
                notified_tx.send(()).expect("試験の通知受信口");
            }),
            1024,
            4,
        )
        .unwrap();
        doc.persistent_filter.toggle_enabled();
        doc.persistent_filter
            .add_operation(FilterOperation::InvertColors);
        doc.open_folder(&dir).unwrap();
        assert_eq!(doc.current_image().unwrap().data, vec![245, 235, 225, 255]);
        assert_eq!(started_rx.recv_timeout(Duration::from_secs(5)).unwrap(), 0);
        let old_generation = doc.prefetch_coord.generation().unwrap();

        doc.toggle_persistent_filter();
        assert!(doc.prefetch_coord.generation().unwrap() > old_generation);
        assert_eq!(doc.current_image().unwrap().data, vec![10, 20, 30, 255]);

        assert!(doc.file_list.navigate_to(1));
        doc.current_image = None;
        for _ in event_rx.try_iter() {}
        release_tx.send(()).unwrap();
        notified_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(started_rx.recv_timeout(Duration::from_secs(5)).unwrap(), 1);
        doc.process_prefetch_responses();
        assert!(
            doc.current_image().is_none(),
            "旧フィルタの画像を表示しない"
        );
        assert!(
            !event_rx
                .try_iter()
                .any(|event| matches!(event, DocumentEvent::ImageReady))
        );

        release_tx.send(()).unwrap();
        notified_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        doc.process_prefetch_responses();
        assert_eq!(doc.file_list().current_index(), Some(1));
        assert_eq!(doc.current_image().unwrap().data, vec![10, 20, 30, 255]);
        assert!(
            event_rx
                .try_iter()
                .any(|event| matches!(event, DocumentEvent::ImageReady))
        );
    }
}
