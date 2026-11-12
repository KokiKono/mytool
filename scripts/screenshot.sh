#!/usr/bin/env bash
# mytool を起動してウィンドウだけを撮る。エージェントが見た目を自分で確かめるために使う。
#   scripts/screenshot.sh <出力 PNG> [起動を待つ秒数]
# 端末アプリに「画面収録」の許可が要る（初回だけ人が許可する）。
set -Eeuo pipefail

out="${1:?出力先の PNG を指定する}"
wait_sec="${2:-4}"
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cargo build --quiet --manifest-path "$repo/Cargo.toml" -p mytool
"${CARGO_TARGET_DIR:-$repo/target}/debug/mytool" &
pid=$!
trap 'kill "$pid" 2>/dev/null || true' EXIT
sleep "$wait_sec"

window_id="$(swift - "$pid" <<'SWIFT'
import CoreGraphics
let pid = Int32(CommandLine.arguments[1])!
// 画面収録の許可が無いと onScreen の情報が欠けるので全件から探す。
// GPUI は見えない補助ウィンドウも持つので、面積が最大のものを本体とみなす。
// デモ起動は前面に出ないパネル（ポップアップの階層）なので、階層では絞らない。
let list = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as! [[String: Any]]
func area(_ w: [String: Any]) -> Double {
    let b = w[kCGWindowBounds as String] as? [String: Double] ?? [:]
    return (b["Width"] ?? 0) * (b["Height"] ?? 0)
}
let mine = list.filter { ($0[kCGWindowOwnerPID as String] as? Int32) == pid }
if let w = mine.max(by: { area($0) < area($1) }) {
    print(w[kCGWindowNumber as String] as! Int)
}
SWIFT
)"
if [[ -z "$window_id" ]]; then
  echo "mytool のウィンドウが見つからない（起動に失敗したか、待ち時間が足りない）" >&2
  exit 1
fi
mkdir -p "$(dirname "$out")"
screencapture -o -x -l"$window_id" "$out"
echo "$out"
