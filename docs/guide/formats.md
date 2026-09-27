# 対応フォーマット

対応する画像・アーカイブ・プラグインのフォーマット一覧。

## 画像 (標準対応)

JPEG、PNG、GIF、BMP、WebP

EXIFの向き情報を持つ画像は、その向きに従って表示する（[画像の表示](./viewing.md#写真の向き)を参照）。

## PDF

Windows.Data.Pdf APIによるPDFレンダリングに対応している。
各ページを画像として表示し、フォルダ内と同様にページ間を移動できる。

## アーカイブ

ZIP / cbz, RAR / cbr, 7z

## Susieプラグイン

64bit Susieプラグイン (.sph / .spi) に対応している。
実行ファイルと同じディレクトリの `spi/` フォルダにプラグインDLLを配置すると自動検出される。

```text
ぐらびゅ.exe
spi/
  ifXXX.sph    ← 画像プラグイン
  axXXX.spi    ← アーカイブプラグイン
```
