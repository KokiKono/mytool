#!/usr/bin/env bash
# 禁止語リスト（~/.config/mytool/deny.txt）を GitHub の Actions secret MYTOOL_DENYLIST に写す。
# pre-push から毎回呼ばれ、前回写したときから中身が変わっていなければ何もしない。
# push より先に写すのは、その push で走る CI に新しいリストを効かせるため。
set -Eeuo pipefail

deny="${MYTOOL_DENYLIST_FILE:-$HOME/.config/mytool/deny.txt}"
stamp="$HOME/.config/mytool/.deny.synced"
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

[[ -f "$deny" ]] || exit 0
if ! command -v gh >/dev/null 2>&1 || ! gh auth status >/dev/null 2>&1; then
  echo "sync-denylist: gh が無いか未認証なので、禁止語リストを secret に写せなかった" >&2
  exit 0
fi

# リモートの owner/repo を origin の URL から取る
slug="$(git -C "$repo" remote get-url origin | sed -E 's#(\.git)?$##; s#^.*github\.com[:/]##')"
body="$(grep -vE '^[[:space:]]*(#|$)' "$deny" || true)"
hash="$(printf '%s\n%s' "$slug" "$body" | shasum -a 256 | cut -d' ' -f1)"
[[ -f "$stamp" && "$(cat "$stamp")" == "$hash" ]] && exit 0

if [[ -n "$body" ]]; then
  printf '%s\n' "$body" | gh secret set MYTOOL_DENYLIST -R "$slug" >/dev/null
  echo "sync-denylist: 禁止語リスト（$(wc -l <<<"$body" | tr -d ' ') 語）を $slug の secret に写した" >&2
else
  gh secret delete MYTOOL_DENYLIST -R "$slug" >/dev/null 2>&1 || true
fi
mkdir -p "$(dirname "$stamp")"
echo "$hash" >"$stamp"
