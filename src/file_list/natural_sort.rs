use std::cmp::Ordering;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use windows::Win32::UI::Shell::StrCmpLogicalW;
use windows::core::PCWSTR;

pub(super) fn compare_explorer(a: &str, b: &str) -> Ordering {
    let wa = to_wide_null(a);
    let wb = to_wide_null(b);
    // SAFETY: `wa` / `wb` はいずれも末尾 `0` 付きの有効な UTF-16 バッファで、
    // 関数呼び出し中は所有権を保持しているため、`PCWSTR` の指す先は live である。
    // `StrCmpLogicalW` は読み取りのみで副作用を持たない。
    let result = unsafe { StrCmpLogicalW(PCWSTR(wa.as_ptr()), PCWSTR(wb.as_ptr())) };
    result.cmp(&0)
}

fn to_wide_null(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(input: &[&str]) -> Vec<String> {
        let mut v: Vec<String> = input.iter().map(|s| (*s).to_string()).collect();
        v.sort_by(|a, b| compare_explorer(a, b));
        v
    }

    #[test]
    fn leading_zero_user_scenario() {
        let result = sorted(&["020.jpg", "018.jpg", "19.jpg"]);
        assert_eq!(result, vec!["018.jpg", "19.jpg", "020.jpg"]);
    }

    #[test]
    fn leading_zero_with_three_digit_neighbor() {
        let result = sorted(&["019.jpg", "020.jpg", "099.jpg", "19.jpg", "100.jpg"]);
        assert_eq!(
            result,
            vec!["019.jpg", "19.jpg", "020.jpg", "099.jpg", "100.jpg"]
        );
    }

    #[test]
    fn case_insensitive_mixed() {
        let result = sorted(&["Img10.png", "img2.png", "IMG1.png"]);
        assert_eq!(result, vec!["IMG1.png", "img2.png", "Img10.png"]);
        assert_eq!(compare_explorer("IMG2.png", "img2.png"), Ordering::Equal);
    }

    #[test]
    fn empty_string_compares_less() {
        assert_eq!(compare_explorer("", "a"), Ordering::Less);
        assert_eq!(compare_explorer("a", ""), Ordering::Greater);
        assert_eq!(compare_explorer("", ""), Ordering::Equal);
    }

    #[test]
    fn alphanumeric_mixed() {
        let result = sorted(&["a10", "a2", "a1"]);
        assert_eq!(result, vec!["a1", "a2", "a10"]);
    }

    #[test]
    fn path_separator_mixed() {
        let result = sorted(&["foo/bar10", "foo/bar2", "foo/bar1"]);
        assert_eq!(result, vec!["foo/bar1", "foo/bar2", "foo/bar10"]);
    }

    #[test]
    fn pure_numeric_simple_order() {
        let result = sorted(&["10", "2", "1"]);
        assert_eq!(result, vec!["1", "2", "10"]);
    }
}
