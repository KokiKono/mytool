#!/usr/bin/env bash
# HOOTL の 1 回分を起動する。起動したら終わるまで人は触らない。
#   scripts/hootl.sh
# 今の HEAD にある CONDITIONS.md の条件で、回ごとの worktree（../<repo>.hootl-<日時>、ブランチ hootl/<日時>）を
# 作ってそこで走らせる。記録は runs/<日時>/ に残る（gitignore 済み。worktree を消しても残る）。
#
# 初回だけ、HOOTL 専用の claude の設定ディレクトリにログインしておく:
#   CLAUDE_CONFIG_DIR=~/.config/mytool/claude claude auth login
set -Eeuo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo"
# 普段の ~/.claude（個人のルール・スキル・記憶）を読ませない。public リポジトリで作業させるので、
# 私的な文脈はそもそも見せない。claude.ai のコネクタ（メールや Slack）は --strict-mcp-config で外れる
config_dir="${MYTOOL_HOOTL_CLAUDE_CONFIG_DIR:-$HOME/.config/mytool/claude}"

die() {
  echo "hootl: $*" >&2
  exit 1
}

# claude の hook や Bash はこの環境を受け継ぐ。brew の rustup は keg-only で、
# zsh を通らずに起動されると PATH に無いので、ここで 1 回だけ足す
if ! command -v cargo >/dev/null 2>&1; then
  for dir in "$HOME/.cargo/bin" /opt/homebrew/opt/rustup/bin; do
    [[ -x "$dir/cargo" ]] && PATH="$dir:$PATH"
  done
fi

# --- 起動前の検査。ここで落ちるものは、走らせてからエージェントに直させない
for cmd in cargo gitleaks jq git claude caffeinate; do
  command -v "$cmd" >/dev/null 2>&1 || die "$cmd が見つからない"
done
[[ -z "$(git status --porcelain)" ]] ||
  die "未コミットの変更がある。worktree は HEAD から作るので、条件書を含めてコミットしてから起動する"
deny="${MYTOOL_DENYLIST_FILE:-$HOME/.config/mytool/deny.txt}"
grep -qvE '^[[:space:]]*(#|$)' "$deny" 2>/dev/null || die "禁止語リスト $deny が空（scripts/setup.sh）"
CLAUDE_CONFIG_DIR="$config_dir" claude auth status >/dev/null 2>&1 ||
  die "HOOTL 用の claude にログインしていない: CLAUDE_CONFIG_DIR=$config_dir claude auth login"

# 上限は CONDITIONS.md の「## 上限」から読み、stop-gate.sh が外から守る（エージェントの自己申告に任せない）
limits="$(awk '/^## /{on = ($0 == "## 上限")} on' CONDITIONS.md)"
hours="$(grep -oE '[0-9]+(\.[0-9]+)? *時間' <<<"$limits" | head -1 | grep -oE '[0-9.]+' || true)"
max_verify="$(grep -oE '実行 *[0-9]+ *回' <<<"$limits" | head -1 | grep -oE '[0-9]+' || true)"
[[ -n "$hours" && -n "$max_verify" ]] ||
  die "CONDITIONS.md の「## 上限」から時間と verify.sh の回数を読めない（例: 2 時間、または \`scripts/verify.sh\` の実行 40 回）"

# --- 回ごとの作業場所。普段のチェックアウトを占有せず、複数の回を並べて走らせられる
ts="$(date +%Y%m%d-%H%M%S)"
run_dir="$repo/runs/$ts"
work="$(dirname "$repo")/$(basename "$repo").hootl-$ts"
base="$(git rev-parse HEAD)"
git worktree add --quiet -b "hootl/$ts" "$work" "$base"
mkdir -p "$run_dir/harness"
cp CONDITIONS.md "$run_dir/CONDITIONS.md"
# hook と設定は開始時点の写しを使う。作業場所や普段のチェックアウトを書き換えても終わり方は変わらない
cp scripts/hooks/stop-gate.sh .claude/hootl-settings.json "$run_dir/harness/"

# 専用の設定ディレクトリでは初回の案内と信頼の確認が出て止まるので、済ませた扱いにしておく
claude_json="$config_dir/.claude.json"
[[ -f "$claude_json" ]] || echo '{}' >"$claude_json"
jq --arg p "$work" '.hasCompletedOnboarding = true | .projects[$p].hasTrustDialogAccepted = true' \
  "$claude_json" >"$claude_json.$$"
mv "$claude_json.$$" "$claude_json"

export PATH CLAUDE_CONFIG_DIR="$config_dir"
export MYTOOL_HOOTL=1 MYTOOL_RUN_DIR="$run_dir" MYTOOL_BASE="$base" MYTOOL_LIMIT_VERIFY="$max_verify"
MYTOOL_STARTED_AT="$(date +%s)"
MYTOOL_LIMIT_SEC="$(awk -v h="$hours" 'BEGIN { printf "%d", h * 3600 }')"
export MYTOOL_STARTED_AT MYTOOL_LIMIT_SEC
# 普段のチェックアウトとビルド結果を共有する。worktree ごとに gpui を一から組むと数分かかる
export CARGO_TARGET_DIR="$repo/target"

cd "$work"
echo "hootl: $work（ブランチ hootl/$ts）で起動する。記録は $run_dir" >&2
# 長い回の途中で Mac が眠ると止まるので、claude が動いているあいだはスリープさせない
# --add-dir は値を複数取るので、後ろに別のオプションを置いて区切る
caffeinate -i claude \
  --add-dir "$run_dir" \
  --permission-mode auto \
  --setting-sources project \
  --strict-mcp-config \
  --settings "$run_dir/harness/hootl-settings.json" \
  "CONDITIONS.md を読み、今回の条件を確認してから作業を始めてください。記録は $run_dir に残してください。" || true
# 落ちた・利用上限に来た・人が止めたなど、Stop hook を通らずに終わったときも知らせる
if [[ ! -f "$run_dir/.notified" ]]; then
  osascript -e "display notification \"止まった（Stop hook を通らずに）: $ts\" with title \"mytool HOOTL\"" >/dev/null 2>&1 || true
fi
