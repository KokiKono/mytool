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
  local key="$2" last="" count=0
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
# .gitignore は未追跡の検出を、rust-toolchain.toml などは verify.sh が見る内容を変えられるので含める
guarded=(scripts .claude .githooks .github .gitleaks.toml .gitignore rust-toolchain.toml rustfmt.toml .rustfmt.toml clippy.toml .clippy.toml .cargo
  CONDITIONS.md AGENTS.md CLAUDE.md)
changed="$(
  cd "$work"
  { git diff --name-only "${MYTOOL_BASE:?}" -- "${guarded[@]}"; git ls-files --others --exclude-standard -- "${guarded[@]}"; } | sort -u
)"
[[ -z "$changed" ]] || block "ハーネス（採点器・hook・条件書・指示）は変えない。元に戻すこと（git checkout $MYTOOL_BASE -- <path>）。変える必要があると考えたなら、理由を DECISIONS.md と REPORT.md に書いて人に委ねる。変わっているもの:
$changed" "harness:$(cksum <<<"$changed")"

# 上限に来ていても、終わる条件を満たしていれば普通に終わらせる。差し戻す理由があるときだけ、それを上限の案内に置き換える
limit=""
if [[ -n "${MYTOOL_STARTED_AT:-}" && -n "${MYTOOL_LIMIT_SEC:-}" ]] && (($(date +%s) - MYTOOL_STARTED_AT >= MYTOOL_LIMIT_SEC)); then
  limit="時間（$((MYTOOL_LIMIT_SEC / 60)) 分）"
fi
runs="$({ wc -l <"$run_dir/verify-history.jsonl"; } 2>/dev/null || echo 0)"
if [[ -n "${MYTOOL_LIMIT_VERIFY:-}" ]] && ((runs >= MYTOOL_LIMIT_VERIFY)); then
  limit="verify.sh の実行回数（${MYTOOL_LIMIT_VERIFY} 回）"
fi
deny() {
  [[ -z "$limit" ]] || block "${limit}が上限に来た。受け入れ基準のどこまで満たしたかを REPORT.md に、詰まった点と次に人が決めることを $run_dir/GIVEUP.md に書いて止まる。" limit
  block "$@"
}

# ファイルの有無を先に見る。verify.sh は数分かかり、走るたびに上限の回数にも数えられる
[[ -f "$run_dir/REPORT.md" ]] || deny "$run_dir/REPORT.md が無い。CONDITIONS.md の受け入れ基準ごとに、満たしたか・どのテストやスクリーンショットで確かめたかを書くこと。" report
[[ -f "$run_dir/AUDIT.md" ]] || deny "監査がまだ。auditor サブエージェントを前面で起動し（バックグラウンドにしない）、終わるまで待つ。結果は $run_dir/AUDIT.md に書かせ、指摘があれば直してから終わる。" audit
if ! out="$("$work/scripts/verify.sh" 2>&1)"; then
  # 出力には所要時間が入って毎回変わるので、作業場所の中身（未追跡のファイルの中身も含む）が前回と同じかで「同じ理由」を判断する
  tree="$(cd "$work" && GIT_INDEX_FILE="$run_dir/.gate-index" git add -A && GIT_INDEX_FILE="$run_dir/.gate-index" git write-tree)"
  deny "scripts/verify.sh が通っていない。直してから終わること。末尾:
$(tail -n 40 <<<"$out")" "verify:$tree"
fi
finish
