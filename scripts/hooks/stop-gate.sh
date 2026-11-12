#!/usr/bin/env bash
# HOOTL の Stop hook。終わってよい条件を満たすまで終了を差し戻す。
#   - ハーネス（採点器・hook・条件書・指示）を開始時点から変えていない
#   - verify.sh が全部通る
#   - $MYTOOL_RUN_DIR に REPORT.md（何をしたか）と AUDIT.md（監査役の結果）がある
# 上限に達して諦めるときは GIVEUP.md を書けば止まれる。条件を満たさずに止まる唯一の出口。
# hootl.sh が開始時に runs/<日時>/harness/ へ写したものが呼ばれる。作業場所のこのファイルを変えても効かない。
set -Eeuo pipefail

[[ "${MYTOOL_HOOTL:-}" == 1 ]] || exit 0
run_dir="${MYTOOL_RUN_DIR:?}"
work="${CLAUDE_PROJECT_DIR:?}"
cat >/dev/null # hook の入力 JSON は使わない

# 同じ理由で差し戻し続けると、エージェントが待つだけのターンを重ねて利用上限まで使い切る。
# 続けて MAX_REPEAT 回になったら、差し戻しをやめて GIVEUP.md を書いて止める
MAX_REPEAT="${MYTOOL_GATE_MAX_REPEAT:-5}"
state="$run_dir/.gate"

notify() {
  [[ -f "$run_dir/.notified" ]] && return
  touch "$run_dir/.notified"
  osascript -e "display notification \"$1\" with title \"mytool HOOTL\"" >/dev/null 2>&1 || true
}

finish() {
  rm -f "$state"
  if [[ -f "$run_dir/GIVEUP.md" ]]; then
    notify "止まった（GIVEUP.md）: $(basename "$run_dir")"
  else
    notify "終わった: $(basename "$run_dir")"
  fi
  exit 0
}

block() {
  local key="${2:-$1}" last="" count=0
  [[ -f "$state" ]] && read -r count last <"$state"
  if [[ "$last" == "$key" ]]; then count=$((count + 1)); else count=1; fi
  if ((count >= MAX_REPEAT)); then
    printf '# 同じ理由で %s 回差し戻したので止めた\n\n%s\n' "$count" "$1" >>"$run_dir/GIVEUP.md"
    finish
  fi
  echo "$count $key" >"$state"
  jq -n --arg r "$1" '{decision: "block", reason: $r}'
  exit 0
}

[[ -f "$run_dir/GIVEUP.md" ]] && finish

# 採点器を変えれば何でも通せてしまうので、判定の前に見る。Bash で書き換えた場合もここで分かる
guarded=(scripts .claude .githooks .github .gitleaks.toml CONDITIONS.md AGENTS.md CLAUDE.md)
changed="$(
  cd "$work"
  { git diff --name-only "${MYTOOL_BASE:?}" -- "${guarded[@]}"; git ls-files --others --exclude-standard -- "${guarded[@]}"; } | sort -u
)"
[[ -z "$changed" ]] || block "ハーネス（採点器・hook・条件書・指示）は変えない。元に戻すこと（git checkout $MYTOOL_BASE -- <path>）。変える必要があると考えたなら、理由を DECISIONS.md と REPORT.md に書いて人に委ねる。変わっているもの:
$changed" "harness:$(cksum <<<"$changed")"

started="${MYTOOL_STARTED_AT:-}"
if [[ -n "$started" && -n "${MYTOOL_LIMIT_HOURS:-}" ]] &&
  awk -v e="$(($(date +%s) - started))" -v h="$MYTOOL_LIMIT_HOURS" 'BEGIN { exit !(e >= h * 3600) }'; then
  block "時間の上限（${MYTOOL_LIMIT_HOURS} 時間）に来た。受け入れ基準のどこまで満たしたかを REPORT.md に、詰まった点と次に人が決めることを $run_dir/GIVEUP.md に書いて止まる。" limit
fi
runs="$({ wc -l <"$run_dir/verify-history.jsonl"; } 2>/dev/null || echo 0)"
if [[ -n "${MYTOOL_LIMIT_VERIFY:-}" ]] && ((runs >= MYTOOL_LIMIT_VERIFY)); then
  block "verify.sh の実行が上限（${MYTOOL_LIMIT_VERIFY} 回）に来た。受け入れ基準のどこまで満たしたかを REPORT.md に、詰まった点と次に人が決めることを $run_dir/GIVEUP.md に書いて止まる。" limit
fi

if ! out="$("$work/scripts/verify.sh" 2>&1)"; then
  block "scripts/verify.sh が通っていない。直してから終わること。末尾:
$(tail -n 40 <<<"$out")" "verify:$(tail -n 40 <<<"$out" | grep -v '"at"' | cksum)"
fi
[[ -f "$run_dir/REPORT.md" ]] || block "$run_dir/REPORT.md が無い。CONDITIONS.md の受け入れ基準ごとに、満たしたか・どのテストやスクリーンショットで確かめたかを書くこと。" report
[[ -f "$run_dir/AUDIT.md" ]] || block "監査がまだ。auditor サブエージェントを前面で起動し（バックグラウンドにしない）、終わるまで待つ。結果は $run_dir/AUDIT.md に書かせ、指摘があれば直してから終わる。" audit
finish
