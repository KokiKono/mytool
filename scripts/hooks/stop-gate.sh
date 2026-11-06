#!/usr/bin/env bash
# HOOTL の Stop hook。終わってよい条件を満たすまで終了を差し戻す。
#   - verify.sh が全部通る
#   - $MYTOOL_RUN_DIR に REPORT.md（何をしたか）と AUDIT.md（監査役の結果）がある
# 上限に達して諦めるときは GIVEUP.md を書けば止まれる。条件を満たさずに止まる唯一の出口。
set -Eeuo pipefail

[[ "${MYTOOL_HOOTL:-}" == 1 ]] || exit 0
run_dir="${MYTOOL_RUN_DIR:?}"
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cat >/dev/null # hook の入力 JSON は使わない

block() {
  jq -n --arg r "$1" '{decision: "block", reason: $r}'
  exit 0
}

[[ -f "$run_dir/GIVEUP.md" ]] && exit 0

if ! out="$("$repo/scripts/verify.sh" 2>&1)"; then
  block "scripts/verify.sh が通っていない。直してから終わること。末尾:
$(tail -n 40 <<<"$out")"
fi
[[ -f "$run_dir/REPORT.md" ]] || block "$run_dir/REPORT.md が無い。CONDITIONS.md の受け入れ基準ごとに、満たしたか・どのテストやスクリーンショットで確かめたかを書くこと。"
[[ -f "$run_dir/AUDIT.md" ]] || block "監査がまだ。auditor サブエージェントを起動し、結果を $run_dir/AUDIT.md に書かせること。指摘があれば直してから終わる。"
exit 0
