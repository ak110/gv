use crate::file_info::FileInfo;
use crate::file_list::natural_sort::compare_explorer;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    Name,
    #[serde(rename = "name_nocase")]
    NameNoCase,
    Size,
    Date,
    #[default]
    Natural,
}

impl SortOrder {
    pub(crate) fn compare(self, a: &FileInfo, b: &FileInfo) -> std::cmp::Ordering {
        let path_a = a.source.display_path();
        let path_b = b.source.display_path();
        match self {
            SortOrder::Name => path_a.cmp(&path_b),
            SortOrder::NameNoCase => path_a.to_lowercase().cmp(&path_b.to_lowercase()),
            SortOrder::Size => a
                .file_size
                .cmp(&b.file_size)
                .then_with(|| path_a.cmp(&path_b)),
            SortOrder::Date => a
                .modified
                .cmp(&b.modified)
                .then_with(|| path_a.cmp(&path_b)),
            SortOrder::Natural => compare_explorer(&path_a, &path_b),
        }
    }
}
