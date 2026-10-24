# ぐらびゅ

[![CI][ci-badge]][ci-url]
[![Release][release-badge]][release-url]

[ci-badge]: https://github.com/ak110/gv/actions/workflows/ci.yaml/badge.svg
[ci-url]: https://github.com/ak110/gv/actions/workflows/ci.yaml
[release-badge]: https://github.com/ak110/gv/actions/workflows/release.yaml/badge.svg
[release-url]: https://github.com/ak110/gv/actions/workflows/release.yaml

Windows用画像ビューアー。

## 特徴

- 前後画像の先読み（表示中の前後の画像を事前に読み込んでおく）による高速切り替え
- 多形式対応: JPEG / PNG / GIF / BMP / WebP / PDF / ZIP / cbz / RAR / cbr / 7z。
  64bit Susieプラグインを任意で追加すると他の形式も開ける（[対応フォーマット](https://ak110.github.io/gv/guide/formats)）
- 設定ファイルとキーバインドによるカスタマイズ

## インストール

[Releases](https://github.com/ak110/gv/releases)からZIPをダウンロードし、任意のフォルダに展開。

## 設定

`ぐらびゅ.default.toml`をコピーして`ぐらびゅ.toml`にリネームすることで設定をカスタマイズできる。
キーバインドは`ぐらびゅ.keys.default.toml`をコピーして`ぐらびゅ.keys.toml`にリネームして編集。

## ドキュメント

- <https://ak110.github.io/gv/> — 概要・使い方
- [docs/development/development.md](docs/development/development.md) — 開発者向け情報
