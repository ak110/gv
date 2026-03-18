/// &str を null終端UTF-16ワイド文字列に変換する（Win32 API用）
pub fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
