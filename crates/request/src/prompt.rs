//! AI に渡す指示と、返ってきた条件書の後始末。

/// `--system-prompt` に渡す。`--resume` で前回の指示が引き継がれる保証が無いので、毎回渡す。
pub const SYSTEM: &str = "\
あなたは、macOS 用の常駐ツール mytool に足したい機能の要望を聞き取り、\
AI エージェントにそのまま渡せる「条件書」に仕上げる聞き手です。\
条件書を受け取ったエージェントは人に質問できないまま実装し、受け入れ基準を自分で確かめて終わります。

条件書の形:
# 今回の条件: <要望の題>
## ゴール
## 受け入れ基準（番号付き）
## やらないこと
## 分かっていること（任意）
## 上限（作業時間と scripts/verify.sh の実行回数）

進め方:
- 曖昧な点・決まっていない点は推測で埋めず、質問で返す。1 回の返答で聞くのは 3 つまで、答えやすい順に並べる
- 受け入れ基準は、テスト・コマンドの終了コード・ファイルの有無・スクリーンショットのような機械で判定できる文にする。\
「使いやすい」「速い」「適切に」のように判定できない語が出たら、操作と結果の形に言い換える案を出して確かめる
- 要望に無い機能は受け入れ基準に入れない。足したくなったものは「やらないこと」へ回す
- 要望どうしの矛盾や、実装を大きく重くする点に気づいたら指摘する
- 返答は短い素のテキストで書く。表や太字は使わない
- あなたはツールを使えない。ファイルを読んだり書いたりせず、やり取りだけで進める";

/// 「条件書にする」で送る指示。
pub const DRAFT: &str = "\
ここまでのやり取りから条件書を書いてください。
- 出力は条件書の Markdown だけにする。前置き・後書き・コードフェンスは付けない
- 見出しは「# 今回の条件: 」「## ゴール」「## 受け入れ基準」「## やらないこと」「## 上限」をこの順で必ず入れる
- 受け入れ基準は番号付きで、1 つずつ機械で判定できる文にする
- まだ決まっていない点は推測で埋めず、「## 分かっていること」に未決として書く";

/// 条件書に必ず入る見出し。
const REQUIRED_SECTIONS: [&str; 5] = [
    "# 今回の条件",
    "## ゴール",
    "## 受け入れ基準",
    "## やらないこと",
    "## 上限",
];

/// 返答全体が 1 つのコードフェンスで囲まれていたら外す。指示しても付けてくることがある。
pub fn strip_fence(text: &str) -> String {
    let trimmed = text.trim();
    let body = if let Some(rest) = trimmed.strip_prefix("```")
        && let Some(inner) = rest.strip_suffix("```")
        && let Some((_lang, inner)) = inner.split_once('\n')
        && !inner.contains("\n```")
    {
        inner.trim()
    } else {
        trimmed
    };
    format!("{body}\n")
}

/// 条件書に足りない見出しを返す。
pub fn missing_sections(markdown: &str) -> Vec<&'static str> {
    REQUIRED_SECTIONS
        .into_iter()
        .filter(|heading| {
            !markdown
                .lines()
                .any(|line| line.trim_end().starts_with(heading))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_prompt_gives_the_role_and_the_three_rules() {
        assert!(SYSTEM.contains("条件書"));
        assert!(SYSTEM.contains("質問で返す"));
        assert!(SYSTEM.contains("機械で判定できる"));
        assert!(SYSTEM.contains("「やらないこと」へ回す"));
        for heading in ["ゴール", "受け入れ基準", "やらないこと", "上限"] {
            assert!(SYSTEM.contains(heading), "{heading}");
        }
    }

    #[test]
    fn strips_a_fence_around_the_whole_reply() {
        assert_eq!(strip_fence("```markdown\n# a\n\nb\n```\n"), "# a\n\nb\n");
        assert_eq!(strip_fence("```\n# a\n```"), "# a\n");
    }

    #[test]
    fn keeps_fences_inside_the_document() {
        let doc = "# a\n\n```sh\nls\n```\n";
        assert_eq!(strip_fence(doc), doc);
        let two = "```\nx\n```\n\n```\ny\n```";
        assert_eq!(strip_fence(two), format!("{two}\n"));
    }

    #[test]
    fn finds_missing_sections() {
        let full = "# 今回の条件: x\n## ゴール\n## 受け入れ基準\n1. a\n## やらないこと\n## 上限\n";
        assert!(missing_sections(full).is_empty());
        assert_eq!(
            missing_sections("# 今回の条件: x\n## ゴール\n"),
            ["## 受け入れ基準", "## やらないこと", "## 上限"]
        );
    }
}
