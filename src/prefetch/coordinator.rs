use crate::file_info::{FileInfo, FileSource};
use crate::image::DecodedImage;
use crate::persistent_filter::PersistentFilter;
use crate::prefetch::loader_thread::PrefetchEngine;
use crate::prefetch::page_cache::PageCache;

pub enum PrefetchEvent {
    CurrentImageReady(DecodedImage),
    Error(String),
}

pub struct PrefetchCoordinator {
    cache: PageCache,
    engine: Option<PrefetchEngine>,
    cache_backward: usize,
    cache_forward: usize,
}

impl PrefetchCoordinator {
    pub fn new() -> Self {
        Self {
            cache: PageCache::new(0),
            engine: None,
            cache_backward: 0,
            cache_forward: 0,
        }
    }

    pub fn start(&mut self, engine: PrefetchEngine, cache_budget: usize, base_image_size: usize) {
        self.engine = Some(engine);
        self.update_cache_range(cache_budget, base_image_size);
    }

    pub fn update_cache_range(&mut self, cache_budget: usize, base_image_size: usize) {
        const MAX_CACHE_FORWARD: usize = 4;
        const MAX_CACHE_BACKWARD: usize = 2;

        let base = base_image_size.max(1);
        let total_slots = (cache_budget / base).max(3);
        self.cache_forward = (total_slots * 2 / 3).clamp(1, MAX_CACHE_FORWARD);
        self.cache_backward = (total_slots / 3).clamp(1, MAX_CACHE_BACKWARD);
        self.cache.set_max_memory(cache_budget);
    }

    pub fn invalidate(&mut self) {
        self.cache.clear();
        if let Some(engine) = &mut self.engine {
            engine.advance_generation();
        }
    }

    pub fn reschedule(&mut self, center: usize, files: &[FileInfo], filter: &PersistentFilter) {
        let Some(engine) = &mut self.engine else {
            return;
        };

        self.cache
            .evict_outside(center, self.cache_backward, self.cache_forward);
        engine.advance_generation();
        send_prefetch_requests(
            engine,
            &self.cache,
            self.cache_forward,
            self.cache_backward,
            center,
            files,
            filter,
        );
    }

    pub fn invalidate_and_reschedule(
        &mut self,
        center: usize,
        files: &[FileInfo],
        filter: &PersistentFilter,
    ) {
        let Some(engine) = &mut self.engine else {
            self.cache.clear();
            return;
        };

        self.cache.clear();
        engine.advance_generation();
        send_prefetch_requests(
            engine,
            &self.cache,
            self.cache_forward,
            self.cache_backward,
            center,
            files,
            filter,
        );
    }

    pub fn process_responses(
        &mut self,
        current_index: Option<usize>,
        has_current_image: bool,
    ) -> Vec<PrefetchEvent> {
        let Some(engine) = &self.engine else {
            return Vec::new();
        };
        let current_gen = engine.generation();
        let responses = engine.drain_responses();
        let mut events = Vec::new();

        for resp in responses {
            match resp {
                crate::prefetch::LoadResponse::Loaded {
                    index,
                    image,
                    generation,
                } => {
                    if generation != current_gen {
                        continue;
                    }
                    let is_current = current_index == Some(index) && !has_current_image;
                    if is_current {
                        events.push(PrefetchEvent::CurrentImageReady(image));
                    } else {
                        self.cache.insert(index, image);
                    }
                }
                crate::prefetch::LoadResponse::Failed {
                    generation, error, ..
                } => {
                    if generation != current_gen {
                        continue;
                    }
                    events.push(PrefetchEvent::Error(error));
                }
            }
        }

        events
    }

    pub fn take(&mut self, index: usize) -> Option<DecodedImage> {
        self.cache.take(index)
    }

    pub fn contains(&self, index: usize) -> bool {
        self.cache.contains(index)
    }

    #[cfg(test)]
    pub fn insert(&mut self, index: usize, image: DecodedImage) -> bool {
        self.cache.insert(index, image)
    }

    #[cfg(test)]
    pub fn generation(&self) -> Option<u64> {
        self.engine.as_ref().map(PrefetchEngine::generation)
    }
}

fn send_prefetch_requests(
    engine: &PrefetchEngine,
    cache: &PageCache,
    cache_forward: usize,
    cache_backward: usize,
    center: usize,
    files: &[FileInfo],
    filter: &PersistentFilter,
) {
    let len = files.len();
    let max_dist = cache_forward.max(cache_backward);

    for dist in 1..=max_dist {
        let fwd = center + dist;
        if dist <= cache_forward && fwd < len {
            request_file_if_needed(engine, cache, fwd, files, filter);
        }
        if dist <= cache_backward && dist <= center {
            request_file_if_needed(engine, cache, center - dist, files, filter);
        }
    }
}

fn request_file_if_needed(
    engine: &PrefetchEngine,
    cache: &PageCache,
    idx: usize,
    files: &[FileInfo],
    filter: &PersistentFilter,
) {
    if matches!(files[idx].source, FileSource::PendingContainer { .. }) {
        return;
    }
    if cache.contains(idx) {
        return;
    }

    engine.request_load(idx, files[idx].source.clone(), filter.clone());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{
        cache_image as make_image, test_archive_manager, test_decoder, test_zip_buffers,
    };
    use std::sync::Arc;

    #[test]
    fn invalidate_and_reschedule_advances_generation_once() {
        let (notify, ready) = crossbeam_channel::unbounded();
        let engine = PrefetchEngine::new(
            Box::new(move || {
                let _ = notify.send(());
            }),
            test_decoder(),
            Arc::new(test_archive_manager(&Arc::new(
                crate::extension_registry::ExtensionRegistry::new(),
            ))),
            test_zip_buffers(),
        )
        .expect("test PrefetchEngine::new");
        let mut coord = PrefetchCoordinator::new();
        coord.start(engine, 1024, 100);
        let dir = crate::test_helpers::TempDir::new("prefetch_generation");
        let old_path = dir.join("old.png");
        let new_path = dir.join("new.png");
        std::fs::write(&old_path, crate::test_helpers::create_1x1_white_png()).unwrap();
        let new_image = DecodedImage {
            data: vec![40, 80, 120, 255],
            width: 1,
            height: 1,
        };
        std::fs::write(&new_path, crate::image::encode_png(&new_image).unwrap()).unwrap();
        let filter = PersistentFilter::new();
        let mut files = [
            FileInfo::from_path(&old_path).unwrap(),
            FileInfo::from_path(&old_path).unwrap(),
        ];
        let timeout = std::time::Duration::from_secs(5);
        coord.reschedule(0, &files, &filter);
        ready.recv_timeout(timeout).expect("old response queued");
        let before = coord.generation().expect("engine started");

        files[1] = FileInfo::from_path(&new_path).unwrap();
        coord.invalidate_and_reschedule(0, &files, &filter);
        assert_eq!(coord.generation(), Some(before + 1));
        ready.recv_timeout(timeout).expect("new response queued");
        let events = coord.process_responses(Some(1), false);
        let [PrefetchEvent::CurrentImageReady(image)] = events.as_slice() else {
            panic!("expected only the new image, got {} events", events.len());
        };
        assert_eq!((image.width, image.height), (1, 1));
        assert_eq!(image.data, [40, 80, 120, 255]);
    }

    #[test]
    fn invalidate_and_reschedule_without_engine_clears_cache() {
        let mut coord = PrefetchCoordinator::new();
        coord.update_cache_range(1024, 100);
        assert!(coord.insert(0, make_image(100)));
        assert!(coord.contains(0));

        coord.invalidate_and_reschedule(0, &[], &PersistentFilter::new());
        assert!(!coord.contains(0));
        assert!(coord.generation().is_none());
    }

    #[test]
    fn cache_operations() {
        let mut coord = PrefetchCoordinator::new();
        coord.update_cache_range(1024, 100);
        assert!(coord.insert(
            0,
            DecodedImage {
                data: vec![1, 2, 3, 4],
                width: 1,
                height: 1
            }
        ));
        assert!(coord.insert(
            1,
            DecodedImage {
                data: vec![5, 6, 7, 8],
                width: 1,
                height: 1
            }
        ));

        let taken = coord.take(0).expect("requested page");
        assert_eq!((taken.width, taken.height), (1, 1));
        assert_eq!(taken.data, [1, 2, 3, 4]);
        assert!(coord.take(0).is_none());
        assert_eq!(coord.take(1).expect("other page").data, [5, 6, 7, 8]);
    }

    #[test]
    fn invalidate_clears_cache() {
        let mut coord = PrefetchCoordinator::new();
        coord.update_cache_range(1024, 100);
        assert!(coord.insert(0, make_image(100)));
        assert!(coord.insert(1, make_image(100)));
        assert!(coord.contains(0));
        assert!(coord.contains(1));

        coord.invalidate();
        assert!(!coord.contains(0));
        assert!(!coord.contains(1));
    }
}
