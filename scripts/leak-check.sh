#!/usr/bin/env bash
# public リポジトリに載せてはいけないものが追跡対象に入っていないかを見る。
#   1. gitleaks（トークン・鍵などのシークレット）
#   2. 禁止語リスト（社名・社内の固有名詞など）。リスト自体が非公開なのでリポジトリには置かない
#      ローカル: ~/.config/mytool/deny.txt（MYTOOL_DENYLIST_FILE で差し替え可）
#      CI: secret を MYTOOL_DENYLIST 環境変数に改行区切りで渡す
#   3. リストが無くても判定できる形（ホームの絶対パス、メールアドレス、社内ツールの URL）
# 見つけた行の中身は出さず、ファイルと行番号だけを出す。CI のログは公開されるため。
set -Eeuo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo"
failed=0

if ! command -v gitleaks >/dev/null 2>&1; then
  echo "leak-check: gitleaks が無い（brew install gitleaks）" >&2
  exit 2
fi
if ! gitleaks dir . --config .gitleaks.toml --no-banner --redact --log-level warn; then
  echo "leak-check: gitleaks が検出した" >&2
  failed=1
fi

files=()
while IFS= read -r -d '' f; do
  [[ -f "$f" ]] && files+=("$f")
done < <(git ls-files -z --cached --others --exclude-standard)
((${#files[@]} > 0)) || exit "$failed"

# grep -o の結果（file:line:一致）から、許可する一致を除いて位置だけを出す
report() {
  local label="$1" allow="$2"
  shift 2
  local hits
  hits="$(grep -noI "$@" -- "${files[@]}" 2>/dev/null |
    { if [[ -n "$allow" ]]; then grep -vE "$allow"; else cat; fi; } |
    cut -d: -f1,2 | sort -u || true)"
  if [[ -n "$hits" ]]; then
    echo "leak-check: $label" >&2
    sed 's/^/  /' <<<"$hits" >&2
    failed=1
  fi
}

denylist="$(
  if [[ -n "${MYTOOL_DENYLIST:-}" ]]; then
    printf '%s\n' "$MYTOOL_DENYLIST"
  elif [[ -f "${MYTOOL_DENYLIST_FILE:-$HOME/.config/mytool/deny.txt}" ]]; then
    cat "${MYTOOL_DENYLIST_FILE:-$HOME/.config/mytool/deny.txt}"
  fi | grep -vE '^[[:space:]]*(#|$)' || true
)"
if [[ -n "$denylist" ]]; then
  report "禁止語リストの語が入っている" "" -iF -f <(printf '%s\n' "$denylist")
else
  echo "leak-check: 禁止語リストが空（~/.config/mytool/deny.txt に社名などを 1 行 1 語で書く）" >&2
fi

# ~ や $HOME で書けば済むので、絶対パスは一律に止める
report "ホームディレクトリの絶対パスが入っている（~ か \$HOME で書く）" "" -E '/Users/[A-Za-z0-9._-]+'
report "メールアドレスが入っている（例には example.com を使う）" \
  '@(example\.(com|org|net)|users\.noreply\.github\.com|noreply\.anthropic\.com)$' \
  -E '[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}'
report "社内ツールの URL が入っている" "" \
  -E 'https?://([a-z0-9-]+\.)?(slack\.com/archives|notion\.so|notion\.site|atlassian\.net)'

exit "$failed"
