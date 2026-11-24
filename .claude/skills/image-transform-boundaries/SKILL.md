---
name: image-transform-boundaries
description: 画像の範囲抽出・反転・回転・拡縮の画素と寸法を、小さな入力の全列挙と固定幅整数の境界で確かめる。対象コードの変更後に起動する。
verifies:
  - src/filter/transform.rs
  - src/filter/transform/exhaustive.rs
  - src/selection.rs
  - src/image/standard.rs
verified-digest: bbb8c06b6ca4db1d3685841f1f02bd156057e02468042f79e12c5a9ef2e1e180
---

# 画像の幾何変換の境界

`src/filter/transform.rs`と`src/selection.rs`の処理を、実関数を呼ぶテスト
`src/filter/transform/exhaustive.rs`で確かめる。端の欠落や過大な寸法で、保存した画像を壊さないための試験である。

## 再実行

Windowsで次を実行する。Linuxではコンパイルまで行い、合否はWindows CIで判定する。

```powershell
mise exec -- cargo test --locked filter::transform::exhaustive::
mise exec -- cargo test --locked filter::transform::exhaustive:: -- --ignored
```

通常のテストは幅・高さ1〜4の画像16件を使う。画素には位置を表す値を入れる。
範囲抽出の座標と辺長は各-1〜5で、画像ごとに2,401件を試験する。
拡縮の指定寸法は各0〜5で、画像ごとに36件を試験する。補間の画質は拡縮の試験外とする。
反転と直角回転は座標の写像と往復を、任意角回転は0度の恒等性と1・45・89度の画素と寸法を試験する。
任意角回転では外接矩形の左上を基準に、整数の出力座標を逆写像する。元画像の内側では端の色で延長して補間し、外側では透明とする。
補間値は丸めを含めて各チャネル1までの差を許す。寸法や透明性には許容差を置かない。

明示実行側では、任意角の0・90・180・270度、範囲抽出の`i32`境界、および標準デコーダの非厳格な既定確保上限を使った計算式を試験する。
任意角の180度で1×1が2×2になる場合、範囲抽出の加算のオーバーフロー、回転後のバイト数を`u32`で表せない場合を反例候補として扱う。
計算式の境界では画像を確保しない。Susie、PDF、各画像形式に共通の最大寸法をこの試験は保証しない。

## 追従

製品の変更が座標の契約・演算型・有効入力の範囲を変えたかを調べる。
反例が出たら期待値を弱めず、テストの期待式の誤りと実関数の欠陥を分ける。
試験が欠陥を検出することは、任意角回転の判定を最終行と列を除く条件に一時変更し、失敗と復元後の結果を比べて確かめる。

試験を実行して結果を確認した後にだけ、次のコマンドで追従記録を更新する。

```shell
uvx --exclude-newer-package pyfltr=false pyfltr verification-sync --update .claude/skills/image-transform-boundaries/SKILL.md
```
