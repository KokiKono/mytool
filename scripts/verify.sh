#!/usr/bin/env bash
# 採点器。HOOTL のエージェントはこれが全部通るまで終われない（scripts/hooks/stop-gate.sh）。
# MYTOOL_RUN_DIR があれば、結果を $MYTOOL_RUN_DIR/verify.json にも残す。
set -Eeuo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo"

# rustup は Homebrew で keg-only なので、hook のようにログインシェルを通らない起動では PATH に無い
if ! command -v cargo >/dev/null 2>&1; then
  for dir in "$HOME/.cargo/bin" /opt/homebrew/opt/rustup/bin /usr/local/opt/rustup/bin; do
    [[ -x "$dir/cargo" ]] && PATH="$dir:$PATH"
  done
  export PATH
fi

steps=(
  "fmt|cargo fmt --all --check"
  "clippy|cargo clippy --workspace --all-targets --quiet -- -D warnings"
  "test|cargo test --workspace --quiet"
  "leak|scripts/leak-check.sh"
)

results=()
failed=0
for step in "${steps[@]}"; do
  name="${step%%|*}"
  cmd="${step#*|}"
  echo "== $name" >&2
  if bash -c "$cmd" >&2; then
    results+=("{\"step\":\"$name\",\"ok\":true}")
  else
    results+=("{\"step\":\"$name\",\"ok\":false}")
    failed=1
  fi
done

json="{\"at\":\"$(date -u +%Y-%m-%dT%H:%M:%SZ)\",\"ok\":$([[ $failed == 0 ]] && echo true || echo false),\"steps\":[$(IFS=,; echo "${results[*]}")]}"
if [[ -n "${MYTOOL_RUN_DIR:-}" ]]; then
  mkdir -p "$MYTOOL_RUN_DIR"
  echo "$json" >"$MYTOOL_RUN_DIR/verify.json"
  echo "$json" >>"$MYTOOL_RUN_DIR/verify-history.jsonl"
fi
echo "$json"
exit "$failed"
