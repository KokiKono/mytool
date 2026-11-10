#!/usr/bin/env bash
# HOOTL の 1 回分を起動する。起動したら終わるまで人は触らない。
#   scripts/hootl.sh
# 条件は CONDITIONS.md、記録は runs/<日時>/ に残る（gitignore 済み）。
set -Eeuo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo"
run_dir="$repo/runs/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$run_dir"
cp CONDITIONS.md "$run_dir/CONDITIONS.md"

# claude の hook や Bash はこの環境を受け継ぐ。brew の rustup は keg-only で、
# zsh を通らずに起動されると PATH に無いので、ここで 1 回だけ足す
if ! command -v cargo >/dev/null 2>&1; then
  for dir in "$HOME/.cargo/bin" /opt/homebrew/opt/rustup/bin; do
    [[ -x "$dir/cargo" ]] && PATH="$dir:$PATH"
  done
  command -v cargo >/dev/null 2>&1 || { echo "hootl: cargo が見つからない" >&2; exit 1; }
fi

export PATH MYTOOL_HOOTL=1 MYTOOL_RUN_DIR="$run_dir"
exec claude \
  --permission-mode auto \
  --settings "$repo/.claude/hootl-settings.json" \
  "CONDITIONS.md を読み、今回の条件を確認してから作業を始めてください。記録は $run_dir に残してください。"
