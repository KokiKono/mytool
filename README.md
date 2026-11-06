# mytool

macOS 用の個人ツールを 1 本にまとめる常駐アプリ。[GPUI](https://www.gpui.rs/) で書く。
クリップボード履歴・ランチャー・キー設定など、外部アプリに頼っている機能を少しずつ取り込んでいく。

開発は Human-out-of-the-loop（HOOTL）で進める。人が `CONDITIONS.md` に条件を書き、エージェントが
採点器（`scripts/verify.sh`）を通るまで実装・確認・監査を繰り返す。

## 初回にやること

1. `brew install rustup gitleaks`（rustup は keg-only なので `$(brew --prefix rustup)/bin` を PATH に足す）
2. `scripts/setup.sh` — git hook を有効にし、禁止語リスト `~/.config/mytool/deny.txt` を作る。中身（社名など）を書く
3. 端末アプリに「画面収録」を許可する（システム設定 → プライバシーとセキュリティ）。`scripts/screenshot.sh` が使う

## 使うもの

| | |
|---|---|
| `cargo run -p mytool` | 起動 |
| `scripts/verify.sh` | fmt / clippy / test / 漏洩検査 |
| `scripts/screenshot.sh <png>` | 起動してウィンドウを撮る |
| `scripts/hootl.sh` | `CONDITIONS.md` の条件で HOOTL を 1 回走らせる。記録は `runs/<日時>/` |

## public リポジトリとして守ること

社名・社内の固有名詞・個人情報・シークレット・利用者の実データはコミットしない。
`scripts/leak-check.sh` が pre-commit・CI・採点器の 3 か所で止める。CI 用の禁止語リストは、push のたびに
`scripts/sync-denylist.sh` が Actions secret `MYTOOL_DENYLIST` へ写す（中身が変わったときだけ）。詳しくは `AGENTS.md`。
