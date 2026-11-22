use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;

use crate::extension_registry::ExtensionRegistry;
use crate::file_info::{FileInfo, FileSource};

mod groups;
mod natural_sort;
pub mod navigation;
mod random;
mod sort;

pub use navigation::NavigationDirection;
pub use sort::SortOrder;

use groups::{compute_group_layout, group_key};
use random::SimpleRng;

pub fn compare_paths_natural(a: &str, b: &str) -> std::cmp::Ordering {
    natural_sort::compare_explorer(a, b)
}

pub struct FileList {
    files: Vec<FileInfo>,
    current_index: Option<usize>,
    sort_order: SortOrder,
    registry: Arc<ExtensionRegistry>,
}

impl FileList {
    pub fn new(registry: Arc<ExtensionRegistry>) -> Self {
        Self {
            files: Vec::new(),
            current_index: None,
            sort_order: SortOrder::default(),
            registry,
        }
    }

    pub fn set_sort_order(&mut self, order: SortOrder) {
        if self.files.is_empty() {
            self.sort_order = order;
        } else {
            self.sort(order);
        }
    }

    pub fn registry(&self) -> &ExtensionRegistry {
        &self.registry
    }

    pub fn registry_handle(&self) -> Arc<ExtensionRegistry> {
        Arc::clone(&self.registry)
    }

    pub fn populate_from_folder(&mut self, folder: &Path) -> Result<()> {
        self.files.clear();
        self.current_index = None;

        let entries = std::fs::read_dir(folder)?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && self.registry.is_image_extension(name)
                && let Ok(info) = FileInfo::from_path(&path)
            {
                self.files.push(info);
            }
        }

        self.sort(self.sort_order);
        Ok(())
    }

    pub fn populate_single(&mut self, path: &Path) -> Result<()> {
        self.files.clear();
        self.current_index = None;

        let info = FileInfo::from_path(path)?;
        self.files.push(info);
        self.current_index = Some(0);
        Ok(())
    }

    pub fn source_matches(a: &FileSource, b: &FileSource) -> bool {
        match (a, b) {
            (
                FileSource::ArchiveEntry {
                    archive: a1,
                    entry: e1,
                    ..
                },
                FileSource::ArchiveEntry {
                    archive: a2,
                    entry: e2,
                    ..
                },
            ) => a1 == a2 && e1 == e2,
            (
                FileSource::PdfPage {
                    pdf_path: p1,
                    page_index: i1,
                },
                FileSource::PdfPage {
                    pdf_path: p2,
                    page_index: i2,
                },
            ) => p1 == p2 && i1 == i2,
            (
                FileSource::PendingContainer { container_path: c1 },
                FileSource::PendingContainer { container_path: c2 },
            ) => c1 == c2,
            (FileSource::File(a), FileSource::File(b)) => a == b,
            _ => false,
        }
    }

    fn restore_current_position(&mut self, source: &FileSource) {
        self.current_index = self
            .files
            .iter()
            .position(|f| Self::source_matches(&f.source, source))
            .or_else(|| (!self.files.is_empty()).then_some(0));
    }

    pub fn set_current_by_path(&mut self, path: &Path) -> bool {
        if let Some(idx) = self
            .files
            .iter()
            .position(|f| f.source.file_path() == Some(path))
        {
            self.current_index = Some(idx);
            true
        } else {
            false
        }
    }

    pub fn navigate_relative(&mut self, offset: isize) -> bool {
        let len = self.files.len();
        if len == 0 {
            return false;
        }
        let current = self.current_index.unwrap_or(0);

        let mut target = ((current as isize + offset) % len as isize + len as isize) as usize % len;

        let step: isize = if offset >= 0 { 1 } else { -1 };

        let mut attempts = 0;
        while self.files[target].load_failed && attempts < len {
            target = ((target as isize + step) % len as isize + len as isize) as usize % len;
            attempts += 1;
        }
        if attempts >= len {
            return false;
        }

        if target == current {
            return false;
        }
        self.current_index = Some(target);
        true
    }

    pub fn navigate_to(&mut self, index: usize) -> bool {
        if index < self.files.len() && self.current_index != Some(index) {
            self.current_index = Some(index);
            true
        } else {
            false
        }
    }

    pub fn navigate_first(&mut self) -> bool {
        self.navigate_to(0)
    }

    pub fn navigate_last(&mut self) -> bool {
        if self.files.is_empty() {
            return false;
        }
        self.navigate_to(self.files.len() - 1)
    }

    pub fn sort(&mut self, order: SortOrder) {
        self.with_position_preserved(|this| {
            this.files.sort_by(|a, b| order.compare(a, b));
            this.sort_order = order;
        });
    }

    pub fn mark_failed(&mut self, index: usize) {
        if let Some(info) = self.files.get_mut(index) {
            info.load_failed = true;
        }
    }

    pub fn clear_failed(&mut self) {
        for info in &mut self.files {
            info.load_failed = false;
        }
    }

    pub fn push(&mut self, info: FileInfo) {
        self.files.push(info);
    }

    pub fn clear(&mut self) {
        self.files.clear();
        self.current_index = None;
    }

    pub fn is_pending_at(&self, index: usize) -> bool {
        self.files
            .get(index)
            .is_some_and(|f| f.source.is_pending_container())
    }

    pub fn has_pending(&self) -> bool {
        self.files.iter().any(|f| f.source.is_pending_container())
    }

    pub fn pending_container_path_at(&self, index: usize) -> Option<PathBuf> {
        self.files.get(index).and_then(|f| match &f.source {
            FileSource::PendingContainer { container_path } => Some(container_path.clone()),
            _ => None,
        })
    }

    pub fn expand_container_at(
        &mut self,
        index: usize,
        entries: Vec<FileInfo>,
        direction: NavigationDirection,
    ) {
        if index >= self.files.len() {
            return;
        }

        let order = self.sort_order;
        let mut entries = entries;
        entries.sort_by(|a, b| order.compare(a, b));

        let entries_len = entries.len();
        let current = self.current_index;

        self.files.splice(index..=index, entries);

        self.current_index = match current {
            None => None,
            _ if self.files.is_empty() => None,
            Some(c) if c < index => Some(c),
            Some(c) if c == index => {
                if entries_len == 0 {
                    Some(c.min(self.files.len() - 1))
                } else {
                    Some(match direction {
                        NavigationDirection::Forward => index,
                        NavigationDirection::Backward => index + entries_len - 1,
                    })
                }
            }
            Some(c) => Some(c + entries_len - 1),
        };
    }

    fn with_position_preserved<F: FnOnce(&mut Self)>(&mut self, f: F) {
        let saved = self.current().map(|f| f.source.clone());
        f(self);
        if let Some(source) = saved {
            self.restore_current_position(&source);
        }
    }

    pub fn sort_current(&mut self) {
        self.sort(self.sort_order);
    }

    pub fn shuffle_all(&mut self) {
        if self.files.len() <= 1 {
            return;
        }
        self.with_position_preserved(|this| {
            let len = this.files.len();
            let mut rng = SimpleRng::new();
            for i in (1..len).rev() {
                let j = rng.next_usize(i + 1);
                this.files.swap(i, j);
            }
        });
    }

    pub fn shuffle_groups(&mut self) {
        if self.files.is_empty() {
            return;
        }
        self.with_position_preserved(|this| {
            let mut group_indices: HashMap<groups::GroupKey, usize> = HashMap::new();
            let mut groups: Vec<Vec<FileInfo>> = Vec::new();
            for file in this.files.drain(..) {
                let key = group_key(&file);
                let idx = *group_indices.entry(key).or_insert_with(|| {
                    let idx = groups.len();
                    groups.push(Vec::new());
                    idx
                });
                groups[idx].push(file);
            }

            let glen = groups.len();
            if glen > 1 {
                let mut rng = SimpleRng::new();
                for i in (1..glen).rev() {
                    let j = rng.next_usize(i + 1);
                    groups.swap(i, j);
                }
            }

            for group in groups {
                this.files.extend(group);
            }
        });
    }

    pub fn mark_at(&mut self, index: usize) {
        if let Some(info) = self.files.get_mut(index) {
            info.marked = true;
        }
    }

    pub fn unmark_at(&mut self, index: usize) {
        if let Some(info) = self.files.get_mut(index) {
            info.marked = false;
        }
    }

    pub fn invert_all_marks(&mut self) {
        for info in &mut self.files {
            info.marked = !info.marked;
        }
    }

    pub fn invert_marks_to_here(&mut self) {
        let end = self.current_index.map_or(0, |i| i + 1);
        for info in &mut self.files[..end] {
            info.marked = !info.marked;
        }
    }

    pub fn marked_indices(&self) -> Vec<usize> {
        self.files
            .iter()
            .enumerate()
            .filter(|(_, f)| f.marked)
            .map(|(i, _)| i)
            .collect()
    }

    pub fn marked_count(&self) -> usize {
        self.files.iter().filter(|f| f.marked).count()
    }

    pub fn remove_at(&mut self, index: usize) -> Option<FileInfo> {
        if index >= self.files.len() {
            return None;
        }
        let removed = self.files.remove(index);

        if self.files.is_empty() {
            self.current_index = None;
        } else if let Some(current) = self.current_index {
            if index < current {
                self.current_index = Some(current - 1);
            } else if index == current {
                self.current_index = Some(current.min(self.files.len() - 1));
            }
        }

        Some(removed)
    }

    pub fn remove_marked(&mut self) -> Vec<FileInfo> {
        let current_info = self.current().map(|f| f.source.clone());

        let mut removed = Vec::new();
        let mut kept = Vec::new();
        for info in self.files.drain(..) {
            if info.marked {
                removed.push(info);
            } else {
                kept.push(info);
            }
        }
        self.files = kept;

        if let Some(source) = current_info {
            self.restore_current_position(&source);
        } else {
            self.current_index = if self.files.is_empty() { None } else { Some(0) };
        }

        removed
    }

    pub fn navigate_prev_mark(&mut self) -> bool {
        let current = self.current_index.unwrap_or(0);
        let len = self.files.len();
        if len == 0 {
            return false;
        }
        for offset in 1..len {
            let idx = (current + len - offset) % len;
            if self.files[idx].marked {
                self.current_index = Some(idx);
                return true;
            }
        }
        false
    }

    pub fn navigate_next_mark(&mut self) -> bool {
        let current = self.current_index.unwrap_or(0);
        let len = self.files.len();
        if len == 0 {
            return false;
        }
        for offset in 1..len {
            let idx = (current + offset) % len;
            if self.files[idx].marked {
                self.current_index = Some(idx);
                return true;
            }
        }
        false
    }

    pub fn navigate_prev_folder(&mut self) -> bool {
        if self.files.is_empty() {
            return false;
        }
        let current = self.current_index.unwrap_or(0);
        let (group_first_indices, file_group_idx) = compute_group_layout(&self.files);
        let total_groups = group_first_indices.len();
        if total_groups <= 1 {
            return false;
        }

        let current_group = file_group_idx[current];
        let target_group = if current_group == 0 {
            total_groups - 1
        } else {
            current_group - 1
        };
        let target = group_first_indices[target_group];
        if target == current {
            return false;
        }
        self.current_index = Some(target);
        true
    }

    pub fn navigate_next_folder(&mut self) -> bool {
        if self.files.is_empty() {
            return false;
        }
        let current = self.current_index.unwrap_or(0);
        let (group_first_indices, file_group_idx) = compute_group_layout(&self.files);
        let total_groups = group_first_indices.len();
        if total_groups <= 1 {
            return false;
        }

        let current_group = file_group_idx[current];
        let target_group = (current_group + 1) % total_groups;
        let target = group_first_indices[target_group];
        if target == current {
            return false;
        }
        self.current_index = Some(target);
        true
    }

    pub fn sorted_navigate(&mut self, direction: isize, sort_order: SortOrder) -> bool {
        let Some(current) = self.current_index else {
            return false;
        };
        if self.files.is_empty() {
            return false;
        }

        let mut sorted_indices: Vec<usize> = (0..self.files.len()).collect();
        sorted_indices.sort_by(|&a, &b| sort_order.compare(&self.files[a], &self.files[b]));

        let pos = sorted_indices.iter().position(|&i| i == current);
        let Some(pos) = pos else {
            return false;
        };

        let new_pos = (pos as isize + direction).rem_euclid(sorted_indices.len() as isize) as usize;
        let target = sorted_indices[new_pos];

        if target == current {
            return false;
        }
        self.current_index = Some(target);
        true
    }

    pub fn files(&self) -> &[FileInfo] {
        &self.files
    }

    pub fn current(&self) -> Option<&FileInfo> {
        self.current_index.and_then(|i| self.files.get(i))
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    pub fn sort_order(&self) -> SortOrder {
        self.sort_order
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn unsupported_standard_formats_need_plugin_registration() {
        let dir = crate::test_helpers::TempDir::new("supported_formats");
        let unsupported = [".tiff", ".tif", ".tga", ".ico", ".cur"];
        for extension in crate::image::StandardDecoder::extensions().chain(unsupported) {
            std::fs::write(dir.join(format!("image{extension}")), b"fixture").unwrap();
        }
        let registry = std::sync::Arc::new(crate::extension_registry::ExtensionRegistry::new());
        let mut list = super::FileList::new(registry.clone());
        list.populate_from_folder(dir.path()).unwrap();
        assert_eq!(
            list.len(),
            crate::image::StandardDecoder::extensions().count()
        );
        for file in list.files() {
            assert!(
                !unsupported
                    .iter()
                    .any(|extension| file.file_name.ends_with(extension))
            );
        }
        let mut plugin_registry = crate::extension_registry::ExtensionRegistry::new();
        plugin_registry.register_image_extensions(&[".tiff".to_string()]);
        let mut list = super::FileList::new(std::sync::Arc::new(plugin_registry));
        list.populate_from_folder(dir.path()).unwrap();
        assert!(
            list.files()
                .iter()
                .any(|file| file.file_name == "image.tiff")
        );
    }
    use super::*;
    use crate::test_helpers::{
        create_test_files, make_archive_file_info, make_file_info_with_source,
        make_pending_container_info, test_registry,
    };

    #[test]
    fn populate_filters_by_extension() {
        let dir = crate::test_helpers::TempDir::new("fl_populate");
        create_test_files(&dir, &["a.jpg", "b.png", "c.txt", "d.bmp", "readme.md"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();

        assert_eq!(fl.len(), 3);
    }

    #[test]
    fn natural_sort_order() {
        let dir = crate::test_helpers::TempDir::new("fl_natural");
        create_test_files(&dir, &["img2.png", "img10.png", "img1.png"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();
        fl.sort(SortOrder::Natural);

        let names: Vec<&str> = fl.files.iter().map(|f| f.file_name.as_str()).collect();
        assert_eq!(names, vec!["img1.png", "img2.png", "img10.png"]);
    }

    #[test]
    fn natural_sort_case_insensitive() {
        let dir = crate::test_helpers::TempDir::new("fl_natural_ci");
        create_test_files(&dir, &["IMG1.png", "img2.png", "Img10.png"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();
        fl.sort(SortOrder::Natural);

        let names: Vec<&str> = fl.files.iter().map(|f| f.file_name.as_str()).collect();
        assert_eq!(names, vec!["IMG1.png", "img2.png", "Img10.png"]);
    }

    #[test]
    fn natural_sort_leading_zero_explorer_order() {
        let dir = crate::test_helpers::TempDir::new("fl_natural_leadzero");
        create_test_files(&dir, &["018.jpg", "19.jpg", "020.jpg"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();
        fl.sort(SortOrder::Natural);

        let names: Vec<&str> = fl.files.iter().map(|f| f.file_name.as_str()).collect();
        assert_eq!(names, vec!["018.jpg", "19.jpg", "020.jpg"]);
    }

    #[test]
    fn navigate_relative_wraps_around() {
        let dir = crate::test_helpers::TempDir::new("fl_nav");
        create_test_files(&dir, &["a.png", "b.png", "c.png"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();
        fl.navigate_to(0);

        assert!(fl.navigate_relative(-1));
        assert_eq!(fl.current_index(), Some(2));

        assert!(fl.navigate_relative(1));
        assert_eq!(fl.current_index(), Some(0));

        fl.navigate_to(1);
        assert!(fl.navigate_relative(1));
        assert_eq!(fl.current_index(), Some(2));
    }

    #[test]
    fn navigate_first_last() {
        let dir = crate::test_helpers::TempDir::new("fl_firstlast");
        create_test_files(&dir, &["a.png", "b.png", "c.png"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();
        fl.navigate_to(1);

        fl.navigate_last();
        assert_eq!(fl.current_index(), Some(2));

        fl.navigate_first();
        assert_eq!(fl.current_index(), Some(0));
    }

    #[test]
    fn set_current_by_path_found() {
        let dir = crate::test_helpers::TempDir::new("fl_setpath");
        create_test_files(&dir, &["a.png", "b.png"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();

        assert!(fl.set_current_by_path(&dir.join("b.png")));
        assert_eq!(fl.current().unwrap().file_name, "b.png");
    }

    #[test]
    fn sort_preserves_current_position() {
        let dir = crate::test_helpers::TempDir::new("fl_sortpreserve");
        create_test_files(&dir, &["c.png", "a.png", "b.png"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();
        fl.set_current_by_path(&dir.join("b.png"));

        fl.sort(SortOrder::Name);
        assert_eq!(fl.current().unwrap().file_name, "b.png");
    }

    #[test]
    fn empty_list_navigation() {
        let mut fl = FileList::new(test_registry());
        assert!(!fl.navigate_relative(1));
        assert!(!fl.navigate_first());
        assert!(!fl.navigate_last());
        assert!(fl.current().is_none());
    }

    #[test]
    fn mark_and_unmark() {
        let dir = crate::test_helpers::TempDir::new("fl_mark");
        create_test_files(&dir, &["a.png", "b.png", "c.png"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();
        fl.navigate_to(0);

        fl.mark_at(1);
        assert!(fl.files[1].marked);
        assert_eq!(fl.marked_count(), 1);

        fl.unmark_at(1);
        assert!(!fl.files[1].marked);
        assert_eq!(fl.marked_count(), 0);
    }

    #[test]
    fn invert_all_marks() {
        let dir = crate::test_helpers::TempDir::new("fl_invert");
        create_test_files(&dir, &["a.png", "b.png", "c.png"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();
        fl.mark_at(0);

        fl.invert_all_marks();
        assert!(!fl.files[0].marked);
        assert!(fl.files[1].marked);
        assert!(fl.files[2].marked);
        assert_eq!(fl.marked_count(), 2);
    }

    #[test]
    fn invert_marks_to_here() {
        let dir = crate::test_helpers::TempDir::new("fl_invert_here");
        create_test_files(&dir, &["a.png", "b.png", "c.png"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();
        fl.navigate_to(1);

        fl.invert_marks_to_here();
        assert!(fl.files[0].marked);
        assert!(fl.files[1].marked);
        assert!(!fl.files[2].marked);
    }

    #[test]
    fn remove_at_adjusts_current_index() {
        let dir = crate::test_helpers::TempDir::new("fl_removeat");
        create_test_files(&dir, &["a.png", "b.png", "c.png"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();
        fl.navigate_to(2);

        fl.remove_at(0);
        assert_eq!(fl.current_index(), Some(1));
        assert_eq!(fl.len(), 2);

        fl.remove_at(1);
        assert_eq!(fl.current_index(), Some(0));
        assert_eq!(fl.len(), 1);
    }

    #[test]
    fn remove_marked() {
        let dir = crate::test_helpers::TempDir::new("fl_removemarked");
        create_test_files(&dir, &["a.png", "b.png", "c.png"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();
        fl.navigate_to(1);
        fl.mark_at(0);
        fl.mark_at(2);

        let removed = fl.remove_marked();
        assert_eq!(removed.len(), 2);
        assert_eq!(fl.len(), 1);
        assert_eq!(fl.current().unwrap().file_name, "b.png");
    }

    #[test]
    fn navigate_marks() {
        let dir = crate::test_helpers::TempDir::new("fl_navmark");
        create_test_files(&dir, &["a.png", "b.png", "c.png", "d.png"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();
        fl.navigate_to(0);
        fl.mark_at(1);
        fl.mark_at(3);

        assert!(fl.navigate_next_mark());
        assert_eq!(fl.current_index(), Some(1));

        assert!(fl.navigate_next_mark());
        assert_eq!(fl.current_index(), Some(3));

        assert!(fl.navigate_next_mark());
        assert_eq!(fl.current_index(), Some(1));

        assert!(fl.navigate_prev_mark());
        assert_eq!(fl.current_index(), Some(3));
    }

    #[test]
    fn sorted_navigate_forward_backward() {
        let dir = crate::test_helpers::TempDir::new("fl_sortnav");
        create_test_files(&dir, &["c.png", "a.png", "b.png"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();
        fl.navigate_to(0);

        assert!(fl.sorted_navigate(1, SortOrder::Natural));
        assert_eq!(fl.current().unwrap().file_name, "b.png");

        assert!(fl.sorted_navigate(1, SortOrder::Natural));
        assert_eq!(fl.current().unwrap().file_name, "c.png");

        assert!(fl.sorted_navigate(1, SortOrder::Natural));
        assert_eq!(fl.current().unwrap().file_name, "a.png");

        assert!(fl.sorted_navigate(-1, SortOrder::Natural));
        assert_eq!(fl.current().unwrap().file_name, "c.png");
    }

    #[test]
    fn navigate_marks_none_marked() {
        let dir = crate::test_helpers::TempDir::new("fl_navmark_none");
        create_test_files(&dir, &["a.png", "b.png"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();
        fl.navigate_to(0);

        assert!(!fl.navigate_next_mark());
        assert!(!fl.navigate_prev_mark());
    }

    #[test]
    fn populate_single_creates_one_entry() {
        let dir = crate::test_helpers::TempDir::new("fl_single");
        create_test_files(&dir, &["target.png", "other.png", "another.jpg"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_single(&dir.join("target.png")).unwrap();

        assert_eq!(fl.len(), 1);
        assert_eq!(fl.current_index(), Some(0));
        assert_eq!(fl.current().unwrap().file_name, "target.png");
    }

    #[test]
    fn sort_natural_orders_archive_entries_with_leading_zero() {
        let registry = test_registry();
        let mut fl = FileList::new(registry);

        fl.push(make_archive_file_info("a.zip", "020.jpg", "020.jpg"));
        fl.push(make_archive_file_info("a.zip", "19.jpg", "19.jpg"));
        fl.push(make_archive_file_info("a.zip", "018.jpg", "018.jpg"));

        fl.sort(SortOrder::Natural);

        let paths: Vec<String> = fl.files.iter().map(|f| f.source.display_path()).collect();
        assert_eq!(
            paths,
            vec![
                "a.zip/018.jpg".to_string(),
                "a.zip/19.jpg".to_string(),
                "a.zip/020.jpg".to_string(),
            ]
        );
    }

    #[test]
    fn set_sort_order_reflects_when_files_present() {
        let registry = test_registry();
        let mut fl = FileList::new(registry);
        fl.push(make_archive_file_info("a.zip", "b.png", "b.png"));
        fl.push(make_archive_file_info("a.zip", "a.png", "a.png"));

        fl.set_sort_order(SortOrder::Name);
        assert_eq!(fl.sort_order(), SortOrder::Name);
        let names: Vec<&str> = fl.files.iter().map(|f| f.file_name.as_str()).collect();
        assert_eq!(names, vec!["a.png", "b.png"]);
    }

    #[test]
    fn set_sort_order_on_empty_list_only_updates_order() {
        let registry = test_registry();
        let mut fl = FileList::new(registry);
        assert_eq!(fl.sort_order(), SortOrder::Natural);
        fl.set_sort_order(SortOrder::Date);
        assert_eq!(fl.sort_order(), SortOrder::Date);
        assert!(fl.files.is_empty());
    }

    #[test]
    fn sort_orders_by_logical_path_across_groups() {
        let registry = test_registry();
        let mut fl = FileList::new(registry);

        fl.push(make_archive_file_info("archive_b.zip", "x.png", "x.png"));
        fl.push(make_archive_file_info("archive_a.zip", "y.png", "y.png"));
        fl.push(make_archive_file_info("archive_a.zip", "z.png", "z.png"));

        fl.sort(SortOrder::Natural);

        let paths: Vec<String> = fl.files.iter().map(|f| f.source.display_path()).collect();
        assert_eq!(
            paths,
            vec![
                "archive_a.zip/y.png".to_string(),
                "archive_a.zip/z.png".to_string(),
                "archive_b.zip/x.png".to_string(),
            ]
        );
    }

    #[test]
    fn shuffle_all_preserves_elements_and_position() {
        let dir = crate::test_helpers::TempDir::new("fl_shuffle_all");
        create_test_files(&dir, &["a.png", "b.png", "c.png", "d.png", "e.png"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();
        fl.set_current_by_path(&dir.join("c.png"));

        fl.shuffle_all();

        let mut names: Vec<String> = fl.files.iter().map(|f| f.file_name.clone()).collect();
        names.sort();
        assert_eq!(names, vec!["a.png", "b.png", "c.png", "d.png", "e.png"]);

        assert_eq!(fl.current().unwrap().file_name, "c.png");
    }

    #[test]
    fn shuffle_groups_preserves_intra_group_order() {
        let registry = test_registry();
        let mut fl = FileList::new(registry);

        fl.push(make_archive_file_info("archive_a.zip", "a1.png", "a1.png"));
        fl.push(make_archive_file_info("archive_a.zip", "a2.png", "a2.png"));
        fl.push(make_archive_file_info("archive_a.zip", "a3.png", "a3.png"));
        fl.push(make_archive_file_info("archive_b.zip", "b1.png", "b1.png"));
        fl.push(make_archive_file_info("archive_b.zip", "b2.png", "b2.png"));

        fl.navigate_to(0);
        fl.shuffle_groups();

        assert_eq!(fl.len(), 5);

        let positions: Vec<usize> = fl
            .files
            .iter()
            .enumerate()
            .filter(|(_, f)| f.file_name.starts_with('a'))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(positions.len(), 3);
        assert_eq!(positions[1] - positions[0], 1);
        assert_eq!(positions[2] - positions[1], 1);
        let a_names: Vec<&str> = positions
            .iter()
            .map(|&i| fl.files[i].file_name.as_str())
            .collect();
        assert_eq!(a_names, vec!["a1.png", "a2.png", "a3.png"]);

        let b_positions: Vec<usize> = fl
            .files
            .iter()
            .enumerate()
            .filter(|(_, f)| f.file_name.starts_with('b'))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(b_positions.len(), 2);
        assert_eq!(b_positions[1] - b_positions[0], 1);
        let b_names: Vec<&str> = b_positions
            .iter()
            .map(|&i| fl.files[i].file_name.as_str())
            .collect();
        assert_eq!(b_names, vec!["b1.png", "b2.png"]);
    }

    #[test]
    fn sorted_navigate_empty_list() {
        let mut fl = FileList::new(test_registry());
        assert!(!fl.sorted_navigate(1, SortOrder::Natural));
        assert!(!fl.sorted_navigate(-1, SortOrder::Name));
    }

    #[test]
    fn sorted_navigate_single_element() {
        let dir = crate::test_helpers::TempDir::new("fl_sortnav_single");
        create_test_files(&dir, &["only.png"]);

        let mut fl = FileList::new(test_registry());
        fl.populate_from_folder(&dir).unwrap();
        fl.navigate_to(0);

        assert!(!fl.sorted_navigate(1, SortOrder::Natural));
        assert!(!fl.sorted_navigate(-1, SortOrder::Natural));
    }

    #[test]
    fn navigate_folder_three_groups() {
        let registry = test_registry();
        let mut fl = FileList::new(registry);

        fl.push(make_archive_file_info("archive_a.zip", "a1.png", "a1.png"));
        fl.push(make_archive_file_info("archive_a.zip", "a2.png", "a2.png"));
        fl.push(make_archive_file_info("archive_b.zip", "b1.png", "b1.png"));
        fl.push(make_archive_file_info("archive_c.zip", "c1.png", "c1.png"));
        fl.push(make_archive_file_info("archive_c.zip", "c2.png", "c2.png"));

        fl.navigate_to(1);
        assert!(fl.navigate_next_folder());
        assert_eq!(fl.current_index(), Some(2));

        assert!(fl.navigate_next_folder());
        assert_eq!(fl.current_index(), Some(3));

        assert!(fl.navigate_next_folder());
        assert_eq!(fl.current_index(), Some(0));

        assert!(fl.navigate_prev_folder());
        assert_eq!(fl.current_index(), Some(3));

        assert!(fl.navigate_prev_folder());
        assert_eq!(fl.current_index(), Some(2));

        assert!(fl.navigate_prev_folder());
        assert_eq!(fl.current_index(), Some(0));
    }

    #[test]
    fn navigate_folder_single_group() {
        let registry = test_registry();
        let mut fl = FileList::new(registry);

        fl.push(make_archive_file_info("only.zip", "x1.png", "x1.png"));
        fl.push(make_archive_file_info("only.zip", "x2.png", "x2.png"));
        fl.navigate_to(0);

        assert!(!fl.navigate_next_folder());
        assert!(!fl.navigate_prev_folder());
    }

    #[test]
    fn navigate_folder_empty_list() {
        let mut fl = FileList::new(test_registry());
        assert!(!fl.navigate_next_folder());
        assert!(!fl.navigate_prev_folder());
    }

    #[test]
    fn navigate_prev_folder_from_middle_of_group() {
        let registry = test_registry();
        let mut fl = FileList::new(registry);

        fl.push(make_archive_file_info("archive_a.zip", "a1.png", "a1.png"));
        fl.push(make_archive_file_info("archive_a.zip", "a2.png", "a2.png"));
        fl.push(make_archive_file_info("archive_a.zip", "a3.png", "a3.png"));
        fl.push(make_archive_file_info("archive_b.zip", "b1.png", "b1.png"));
        fl.push(make_archive_file_info("archive_b.zip", "b2.png", "b2.png"));
        fl.push(make_archive_file_info("archive_b.zip", "b3.png", "b3.png"));

        fl.navigate_to(4);
        assert!(fl.navigate_prev_folder());
        assert_eq!(fl.current_index(), Some(0));
    }

    #[test]
    fn navigate_relative_skips_failed() {
        let registry = test_registry();
        let mut fl = FileList::new(registry);

        fl.push(make_archive_file_info("a.zip", "a.png", "a.png"));
        fl.push(make_archive_file_info("a.zip", "b.png", "b.png"));
        fl.push(make_archive_file_info("a.zip", "c.png", "c.png"));
        fl.navigate_to(0);

        fl.mark_failed(1);

        assert!(fl.navigate_relative(1));
        assert_eq!(fl.current_index(), Some(2));
    }

    #[test]
    fn navigate_relative_all_failed() {
        let registry = test_registry();
        let mut fl = FileList::new(registry);

        fl.push(make_archive_file_info("a.zip", "a.png", "a.png"));
        fl.push(make_archive_file_info("a.zip", "b.png", "b.png"));
        fl.navigate_to(0);

        fl.mark_failed(0);
        fl.mark_failed(1);

        assert!(!fl.navigate_relative(1));
    }

    #[test]
    fn expand_container_at_forward_places_current_at_first() {
        let registry = test_registry();
        let mut fl = FileList::new(registry);
        fl.push(make_archive_file_info("a.zip", "a1.png", "a1.png"));
        fl.push(make_pending_container_info("pending.zip"));
        fl.push(make_archive_file_info("c.zip", "c1.png", "c1.png"));
        fl.navigate_to(1);

        let entries = vec![
            make_archive_file_info("pending.zip", "p1.png", "p1.png"),
            make_archive_file_info("pending.zip", "p2.png", "p2.png"),
            make_archive_file_info("pending.zip", "p3.png", "p3.png"),
        ];
        fl.expand_container_at(1, entries, NavigationDirection::Forward);

        assert_eq!(fl.len(), 5);
        let names: Vec<&str> = fl.files.iter().map(|f| f.file_name.as_str()).collect();
        assert_eq!(
            names,
            vec!["a1.png", "p1.png", "p2.png", "p3.png", "c1.png"]
        );
        assert_eq!(fl.current().unwrap().file_name, "p1.png");
        assert_eq!(fl.current_index(), Some(1));
    }

    #[test]
    fn expand_container_at_backward_places_current_at_last() {
        let registry = test_registry();
        let mut fl = FileList::new(registry);
        fl.push(make_archive_file_info("a.zip", "a1.png", "a1.png"));
        fl.push(make_pending_container_info("pending.zip"));
        fl.push(make_archive_file_info("c.zip", "c1.png", "c1.png"));
        fl.navigate_to(1);

        let entries = vec![
            make_archive_file_info("pending.zip", "p1.png", "p1.png"),
            make_archive_file_info("pending.zip", "p2.png", "p2.png"),
            make_archive_file_info("pending.zip", "p3.png", "p3.png"),
        ];
        fl.expand_container_at(1, entries, NavigationDirection::Backward);

        assert_eq!(fl.len(), 5);
        let names: Vec<&str> = fl.files.iter().map(|f| f.file_name.as_str()).collect();
        assert_eq!(
            names,
            vec!["a1.png", "p1.png", "p2.png", "p3.png", "c1.png"]
        );
        assert_eq!(fl.current().unwrap().file_name, "p3.png");
        assert_eq!(fl.current_index(), Some(3));
    }

    #[test]
    fn expand_container_at_before_current_keeps_current_file() {
        let registry = test_registry();
        let mut fl = FileList::new(registry);
        fl.push(make_pending_container_info("pending.zip"));
        fl.push(make_archive_file_info("c.zip", "c1.png", "c1.png"));
        fl.push(make_archive_file_info("c.zip", "c2.png", "c2.png"));
        fl.navigate_to(2);

        let entries = vec![
            make_archive_file_info("pending.zip", "p1.png", "p1.png"),
            make_archive_file_info("pending.zip", "p2.png", "p2.png"),
            make_archive_file_info("pending.zip", "p3.png", "p3.png"),
        ];
        fl.expand_container_at(0, entries, NavigationDirection::Forward);

        assert_eq!(fl.len(), 5);
        let names: Vec<&str> = fl.files.iter().map(|f| f.file_name.as_str()).collect();
        assert_eq!(
            names,
            vec!["p1.png", "p2.png", "p3.png", "c1.png", "c2.png"]
        );
        assert_eq!(fl.current().unwrap().file_name, "c2.png");
        assert_eq!(fl.current_index(), Some(4));
    }

    #[test]
    fn sort_logical_path_orders_mixed_sources() {
        let registry = test_registry();
        let mut fl = FileList::new(registry);
        let now = std::time::SystemTime::UNIX_EPOCH;

        fl.push(make_file_info_with_source(
            FileSource::PdfPage {
                pdf_path: PathBuf::from("/folder_b/doc.pdf"),
                page_index: 0,
            },
            100,
            now,
        ));
        fl.push(make_file_info_with_source(
            FileSource::File(PathBuf::from("/folder_a/sub/img2.png")),
            100,
            now,
        ));
        fl.push(make_file_info_with_source(
            FileSource::ArchiveEntry {
                archive: PathBuf::from("/folder_b/archive.zip"),
                entry: "inner.png".to_string(),
                on_demand: false,
                temp_path: None,
                entry_index: None,
            },
            100,
            now,
        ));
        fl.push(make_file_info_with_source(
            FileSource::File(PathBuf::from("/folder_a/img1.png")),
            100,
            now,
        ));

        fl.sort(SortOrder::Natural);

        let paths: Vec<String> = fl.files.iter().map(|f| f.source.display_path()).collect();
        assert_eq!(
            paths,
            vec![
                "/folder_a/img1.png".to_string(),
                "/folder_a/sub/img2.png".to_string(),
                "/folder_b/archive.zip/inner.png".to_string(),
                "/folder_b/doc.pdf/Page 1".to_string(),
            ]
        );
    }

    #[test]
    fn sort_size_uses_logical_path_as_tiebreaker() {
        let registry = test_registry();
        let mut fl = FileList::new(registry);
        let now = std::time::SystemTime::UNIX_EPOCH;

        fl.push(make_file_info_with_source(
            FileSource::File(PathBuf::from("/x/c.png")),
            100,
            now,
        ));
        fl.push(make_file_info_with_source(
            FileSource::File(PathBuf::from("/x/b.png")),
            100,
            now,
        ));
        fl.push(make_file_info_with_source(
            FileSource::File(PathBuf::from("/x/a.png")),
            100,
            now,
        ));

        fl.sort(SortOrder::Size);

        let names: Vec<&str> = fl.files.iter().map(|f| f.file_name.as_str()).collect();
        assert_eq!(names, vec!["a.png", "b.png", "c.png"]);
    }

    #[test]
    fn sort_date_uses_logical_path_as_tiebreaker() {
        let registry = test_registry();
        let mut fl = FileList::new(registry);
        let epoch = std::time::SystemTime::UNIX_EPOCH;
        let future = epoch + std::time::Duration::from_secs(100);

        fl.push(make_file_info_with_source(
            FileSource::File(PathBuf::from("/x/c.png")),
            100,
            future,
        ));
        fl.push(make_file_info_with_source(
            FileSource::File(PathBuf::from("/x/b.png")),
            100,
            epoch,
        ));
        fl.push(make_file_info_with_source(
            FileSource::File(PathBuf::from("/x/a.png")),
            100,
            epoch,
        ));

        fl.sort(SortOrder::Date);

        let names: Vec<&str> = fl.files.iter().map(|f| f.file_name.as_str()).collect();
        assert_eq!(names, vec!["a.png", "b.png", "c.png"]);
    }
}
