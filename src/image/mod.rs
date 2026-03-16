mod standard;

pub use standard::StandardDecoder;

/// デコード済み画像データ（RGBAピクセル）
#[allow(dead_code)]
pub struct DecodedImage {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl DecodedImage {
    /// メモリ使用量（バイト）
    #[allow(dead_code)]
    pub fn memory_size(&self) -> usize {
        self.data.len()
    }
}

/// 画像メタデータ
#[allow(dead_code)]
pub struct ImageMetadata {
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub comments: Vec<String>,
}

/// 画像デコーダの共通インターフェース
#[allow(dead_code)]
pub trait ImageDecoder: Send + Sync {
    /// 対応する拡張子のリスト（ドット付き小文字、例: ".jpg"）
    fn supported_extensions(&self) -> &[&str];

    /// バイト列からデコード可能か判定
    fn can_decode(&self, data: &[u8]) -> bool;

    /// デコード実行
    fn decode(&self, data: &[u8]) -> anyhow::Result<DecodedImage>;

    /// メタデータ取得
    fn metadata(&self, data: &[u8]) -> anyhow::Result<ImageMetadata>;
}
