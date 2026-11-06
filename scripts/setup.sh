#!/usr/bin/env bash
# clone したら 1 回だけ叩く。git hook（pre-commit の漏洩検査、pre-push の禁止語リストの同期）を
# 有効にし、禁止語リストの置き場を作る。
set -Eeuo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
git -C "$repo" config core.hooksPath .githooks

deny="$HOME/.config/mytool/deny.txt"
if [[ ! -f "$deny" ]]; then
  mkdir -p "$(dirname "$deny")"
  cat >"$deny" <<'TXT'
# public リポジトリに入ってはいけない語を 1 行 1 語で書く（大文字小文字は区別しない）
# 例: 勤務先の社名、社内のプロダクト名・リポジトリ名、取引先名
TXT
  echo "禁止語リストを作った。中身を書くこと: $deny"
fi
"$repo/scripts/sync-denylist.sh"
