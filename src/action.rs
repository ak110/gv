//! キー入力・メニュー・アプリ層が共有する操作体系。

/// 全操作を列挙するenum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter)]
pub enum Action {
    // --- ナビゲーション ---
    NavigateBack,
    NavigateForward,
    Navigate5Back,
    Navigate5Forward,
    Navigate50Back,
    Navigate50Forward,
    NavigateFirst,
    NavigateLast,
    NavigatePrevFolder,
    NavigateNextFolder,
    NavigatePrevMark,
    NavigateNextMark,
    NavigateToPage,
    SortNavigateBack,
    SortNavigateForward,
    ShuffleAll,
    ShuffleGroups,

    // --- 表示モード ---
    DisplayAutoShrink,
    DisplayAutoFit,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ToggleMargin,
    CycleAlphaBackground,

    // --- ウィンドウ ---
    ToggleFullscreen,
    Minimize,
    ToggleMaximize,
    ToggleAlwaysOnTop,
    ToggleCursorHide,
    ToggleMenuBar,

    // --- ファイル操作 ---
    NewWindow,
    OpenFile,
    OpenFolder,
    CloseAll,
    Reload,
    RemoveFromList,
    DeleteFile,
    MoveFile,
    CopyFile,
    OpenContainingFolder,
    CopyFileName,
    CopyImage,
    PasteImage,
    ExportJpg,
    ExportBmp,
    ExportPng,
    ShowImageInfo,

    // --- マーク操作 ---
    MarkSet,
    MarkUnset,
    MarkInvertAll,
    MarkInvertToHere,
    MarkedRemoveFromList,
    MarkedDelete,
    MarkedMove,
    MarkedCopy,
    MarkedCopyNames,

    // --- 編集 ---
    DeselectSelection,
    Crop,
    FlipHorizontal,
    FlipVertical,
    Rotate180,
    Rotate90CW,
    Rotate90CCW,
    RotateArbitrary,
    Resize,

    // --- フィルタ (画像メニュー) ---
    Fill,
    Levels,
    Gamma,
    BrightnessContrast,
    Mosaic,
    GaussianBlur,
    UnsharpMask,
    InvertColors,
    GrayscaleSimple,
    GrayscaleStrict,
    ApplyAlpha,
    Blur,
    BlurStrong,
    Sharpen,
    SharpenStrong,
    MedianFilter,

    // --- 永続フィルタ ---
    PFilterToggle,
    PFilterFlipH,
    PFilterFlipV,
    PFilterRotate180,
    PFilterRotate90CW,
    PFilterRotate90CCW,
    PFilterLevels,
    PFilterGamma,
    PFilterBrightnessContrast,
    PFilterGrayscaleSimple,
    PFilterGrayscaleStrict,
    PFilterBlur,
    PFilterBlurStrong,
    PFilterSharpen,
    PFilterSharpenStrong,
    PFilterGaussianBlur,
    PFilterUnsharpMask,
    PFilterMedianFilter,
    PFilterInvertColors,
    PFilterApplyAlpha,

    // --- ブックマーク ---
    BookmarkSave,
    BookmarkLoad,

    // --- ファイルリスト ---
    ToggleFileList,

    // --- ユーティリティ ---
    OpenExeFolder,
    OpenBookmarkFolder,
    OpenSpiFolder,
    OpenTempFolder,
    ShowHelp,
    CheckUpdate,
    OpenHomepage,
    RegisterShell,
    UnregisterShell,
    Exit,

    // --- スライドショー ---
    SlideshowToggle,
    SlideshowFaster,
    SlideshowSlower,
}
