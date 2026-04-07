#!/usr/bin/env bash
# Edit/Write/MultiEdit 後に呼ばれる PostToolUse hook。
# - 編集された .rs ファイルに対して `cargo fmt` を実行する。
# - 同ファイルが unsafe ブロックを含む場合、unsafe-reviewer サブエージェントの
#   起動を促すメッセージを exit 2 + stderr で Claude に通知する。
#
# fmt 後のファイル状態同期は Claude Code ハーネスが自動で吸収するため、
# fmt そのものは exit 0 で静かに行う。詳細は plans/lively-gliding-meerkat.md 参照。

set -u

# 1. stdin JSON から file_path を抽出する。
#    jq への依存を避けるため node を使う (mise 経由で必ず利用可能)。
file_path=$(mise exec -- node -e '
  let s = "";
  process.stdin.on("data", c => s += c);
  process.stdin.on("end", () => {
    try {
      const j = JSON.parse(s);
      const p = j.tool_input && j.tool_input.file_path;
      if (p) process.stdout.write(p);
    } catch (_) {}
  });
')

# 2. 非 .rs ファイルは即終了。
case "$file_path" in
    *.rs) ;;
    *) exit 0 ;;
esac

# 3. プロジェクトルートに移動 (このスクリプトは .claude/hooks/ 配下に置かれる)。
cd "$(dirname "$0")/../.." || exit 0

# 4. ファイル実在確認 (Edit が削除/移動された等の極端ケース)。
[ -f "$file_path" ] || exit 0

# 5. cargo fmt をファイル単位で実行。失敗は非ブロッキング。
mise exec -- cargo fmt -- "$file_path" >&2 2>&1 || \
    echo "cargo fmt failed for $file_path (non-blocking)" >&2

# 6. unsafe ブロック検出 → unsafe-reviewer 起動リマインダ。
#    `unsafe trait` / `unsafe fn` / `unsafe { ... }` のいずれにも反応する。
if grep -qE '(^|[[:space:]])unsafe([[:space:]]|\{|$)' "$file_path"; then
    cat >&2 <<EOF
${file_path} contains an unsafe block. Invoke the unsafe-reviewer subagent on this file via the Task tool (subagent_type=unsafe-reviewer) before considering the edit complete. The reviewer validates SAFETY comment usage against the project's CLAUDE.md rules.
EOF
    exit 2
fi

exit 0
