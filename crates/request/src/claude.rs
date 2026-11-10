//! `claude` CLI を相手にする [`Assistant`]。

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::{Ask, AskError, Assistant, Reply, prompt};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(180);

pub struct ClaudeCli {
    pub program: PathBuf,
    /// CLI はセッションを作業ディレクトリごとに覚えるので、`--resume` が効くよう毎回同じ場所で動かす
    pub cwd: PathBuf,
    pub timeout: Duration,
}

impl ClaudeCli {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            program: "claude".into(),
            cwd,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// 発言は引数ではなく標準入力で渡す。`-` で始まる文をオプションと取り違えさせないため。
    pub fn args(ask: &Ask) -> Vec<String> {
        let mut args: Vec<String> = [
            "-p",
            "--output-format",
            "json",
            // 相手は文章を返すだけなので、ツールも利用者の設定（CLAUDE.md・hook）も使わせない
            "--tools",
            "",
            "--setting-sources",
            "",
            "--system-prompt",
            prompt::SYSTEM,
        ]
        .map(String::from)
        .into();
        if let Some(session) = &ask.resume {
            args.extend(["--resume".into(), session.clone()]);
        }
        args
    }
}

impl Assistant for ClaudeCli {
    fn ask(&self, ask: &Ask) -> Result<Reply, AskError> {
        let program = self.program.display().to_string();
        std::fs::create_dir_all(&self.cwd)
            .map_err(|e| AskError::Failed(format!("{} を作れない: {e}", self.cwd.display())))?;
        let mut child = Command::new(&self.program)
            .args(Self::args(ask))
            .current_dir(&self.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| match e.kind() {
                io::ErrorKind::NotFound => AskError::NotFound(program.clone()),
                _ => AskError::Failed(format!("{program} を起動できない: {e}")),
            })?;

        let mut stdin = child.stdin.take().expect("stdin は piped");
        let prompt = ask.prompt.clone();
        // 長い発言でパイプが詰まっても待ち時間の判定を止めないよう、書き込みも別スレッドにする
        thread::spawn(move || stdin.write_all(prompt.as_bytes()));
        let stdout = read_all(child.stdout.take().expect("stdout は piped"));
        let stderr = read_all(child.stderr.take().expect("stderr は piped"));

        let deadline = Instant::now() + self.timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AskError::TimedOut(self.timeout));
                }
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(e) => return Err(AskError::Failed(e.to_string())),
            }
        };
        // claude が終わってもパイプを握った子孫が残ると読み終わらないので、ここも期限内で待つ
        let collect = |rx: mpsc::Receiver<String>| {
            rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        };
        let (Ok(stdout), Ok(stderr)) = (collect(stdout), collect(stderr)) else {
            return Err(AskError::TimedOut(self.timeout));
        };
        parse_output(&stdout, &stderr, status)
    }
}

fn read_all(mut pipe: impl Read + Send + 'static) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf);
        let _ = tx.send(String::from_utf8_lossy(&buf).into_owned());
    });
    rx
}

/// `claude -p --output-format json` の出力を読む。
pub fn parse_output(stdout: &str, stderr: &str, status: ExitStatus) -> Result<Reply, AskError> {
    let json = stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| line.starts_with('{'))
        .and_then(|line| serde_json::from_str::<Value>(line).ok());
    let Some(json) = json else {
        let detail = last_lines(stderr)
            .or_else(|| last_lines(stdout))
            .unwrap_or_else(|| format!("出力が無い（{status}）"));
        return Err(AskError::Failed(detail));
    };

    let text = json.get("result").and_then(Value::as_str);
    let is_error = json.get("is_error").and_then(Value::as_bool) == Some(true)
        || json.get("subtype").and_then(Value::as_str) != Some("success");
    if is_error {
        let detail = text
            .filter(|t| !t.is_empty())
            .or_else(|| json.get("subtype").and_then(Value::as_str))
            .unwrap_or("不明なエラー");
        return Err(AskError::Failed(detail.to_string()));
    }
    match (text, json.get("session_id").and_then(Value::as_str)) {
        (Some(text), Some(session)) => Ok(Reply {
            text: text.to_string(),
            session_id: session.to_string(),
        }),
        _ => Err(AskError::Failed(
            "出力に result か session_id が無い".into(),
        )),
    }
}

fn last_lines(text: &str) -> Option<String> {
    let lines: Vec<_> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    (!lines.is_empty()).then(|| lines[lines.len().saturating_sub(3)..].join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::ExitStatusExt;

    fn ok() -> ExitStatus {
        ExitStatus::from_raw(0)
    }

    fn ask(prompt: &str, resume: Option<&str>) -> Ask {
        Ask {
            prompt: prompt.into(),
            resume: resume.map(String::from),
        }
    }

    #[test]
    fn args_turn_off_tools_and_settings_and_set_the_system_prompt() {
        let args = ClaudeCli::args(&ask("x", None));
        let pairs: Vec<_> = args
            .windows(2)
            .map(|w| (w[0].as_str(), w[1].as_str()))
            .collect();
        assert_eq!(args[0], "-p");
        assert!(pairs.contains(&("--output-format", "json")));
        assert!(pairs.contains(&("--tools", "")));
        assert!(pairs.contains(&("--setting-sources", "")));
        assert!(pairs.contains(&("--system-prompt", prompt::SYSTEM)));
        assert!(!args.contains(&"--resume".to_string()));
        // 発言は引数に入れない
        assert!(!args.contains(&"x".to_string()));
    }

    #[test]
    fn args_resume_the_previous_session() {
        let args = ClaudeCli::args(&ask("x", Some("abc")));
        assert!(args.windows(2).any(|w| w[0] == "--resume" && w[1] == "abc"));
    }

    #[test]
    fn parses_a_successful_result() {
        let out = r#"{"type":"result","subtype":"success","is_error":false,"result":"何を保存しますか？","session_id":"s-1","total_cost_usd":0.01}"#;
        assert_eq!(
            parse_output(out, "", ok()),
            Ok(Reply {
                text: "何を保存しますか？".into(),
                session_id: "s-1".into()
            })
        );
    }

    #[test]
    fn parses_the_last_json_line_when_something_is_printed_before_it() {
        let out = "warning: x\n{\"subtype\":\"success\",\"is_error\":false,\"result\":\"a\",\"session_id\":\"s\"}\n";
        assert_eq!(parse_output(out, "", ok()).unwrap().text, "a");
    }

    #[test]
    fn error_results_become_failures() {
        let out = r#"{"type":"result","subtype":"success","is_error":true,"result":"Not logged in","session_id":"s"}"#;
        assert_eq!(
            parse_output(out, "", ok()),
            Err(AskError::Failed("Not logged in".into()))
        );
        let out =
            r#"{"type":"result","subtype":"error_max_turns","is_error":false,"session_id":"s"}"#;
        assert_eq!(
            parse_output(out, "", ok()),
            Err(AskError::Failed("error_max_turns".into()))
        );
    }

    #[test]
    fn non_json_output_reports_stderr() {
        let got = parse_output(
            "",
            "error: unknown option '--x'\n",
            ExitStatus::from_raw(256),
        );
        assert_eq!(
            got,
            Err(AskError::Failed("error: unknown option '--x'".into()))
        );
    }

    /// 偽の `claude` を置く。本物の CLI は呼ばない。
    fn fake_cli(dir: &std::path::Path, body: &str) -> PathBuf {
        let path = dir.join("fake-claude");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn cli(program: PathBuf, cwd: PathBuf, timeout: Duration) -> ClaudeCli {
        ClaudeCli {
            program,
            cwd,
            timeout,
        }
    }

    #[test]
    fn runs_the_cli_with_the_prompt_on_stdin_in_the_given_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let work = tmp.path().join("work");
        // 標準入力・作業ディレクトリ・--resume の値を返答に詰めて返す
        let program = fake_cli(
            tmp.path(),
            r#"input=$(cat)
resume=""
while [ $# -gt 0 ]; do [ "$1" = "--resume" ] && resume="$2"; shift; done
printf '{"subtype":"success","is_error":false,"result":"%s|%s|%s","session_id":"s-2"}\n' "$input" "$(basename "$PWD")" "$resume""#,
        );
        let cli = cli(program, work, Duration::from_secs(10));

        let reply = cli.ask(&ask("メモ帳", Some("s-1"))).unwrap();

        assert_eq!(reply.text, "メモ帳|work|s-1");
        assert_eq!(reply.session_id, "s-2");
    }

    #[test]
    fn missing_cli_is_reported_as_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let cli = cli(
            tmp.path().join("no-such-claude"),
            tmp.path().into(),
            Duration::from_secs(10),
        );
        assert!(matches!(
            cli.ask(&ask("x", None)),
            Err(AskError::NotFound(_))
        ));
    }

    #[test]
    fn failing_cli_is_reported_with_its_stderr() {
        let tmp = tempfile::tempdir().unwrap();
        let program = fake_cli(tmp.path(), "cat >/dev/null; echo 'boom' >&2; exit 1");
        let cli = cli(program, tmp.path().into(), Duration::from_secs(10));
        assert_eq!(
            cli.ask(&ask("x", None)),
            Err(AskError::Failed("boom".into()))
        );
    }

    #[test]
    fn slow_cli_times_out_and_is_killed() {
        let tmp = tempfile::tempdir().unwrap();
        let program = fake_cli(tmp.path(), "exec sleep 30");
        let cli = cli(program, tmp.path().into(), Duration::from_millis(300));

        let started = Instant::now();
        let got = cli.ask(&ask("x", None));

        assert_eq!(got, Err(AskError::TimedOut(Duration::from_millis(300))));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn a_leftover_child_holding_stdout_does_not_outlast_the_timeout() {
        let tmp = tempfile::tempdir().unwrap();
        // claude 本体はすぐ終わるが、出力のパイプを受け継いだ子が残る
        let program = fake_cli(tmp.path(), "sleep 30 &\necho '{}'");
        let cli = cli(program, tmp.path().into(), Duration::from_millis(300));

        let started = Instant::now();
        let got = cli.ask(&ask("x", None));

        assert_eq!(got, Err(AskError::TimedOut(Duration::from_millis(300))));
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
