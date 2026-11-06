//! 条件書の保存先。要望文に私的な内容が入りうるので、リポジトリの外に置く。

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// mytool のデータの置き場。`MYTOOL_DATA_DIR` があればそれを使う。
pub fn data_dir() -> PathBuf {
    data_dir_from(
        std::env::var_os("MYTOOL_DATA_DIR"),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

fn data_dir_from(env: Option<OsString>, home: Option<PathBuf>) -> PathBuf {
    match env.filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => home
            .unwrap_or_default()
            .join("Library/Application Support/mytool"),
    }
}

pub struct RequestStore {
    dir: PathBuf,
}

impl RequestStore {
    /// `data_dir` の下の `requests/` に保存する。
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join("requests"),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// 条件書を新しいファイルに書き、そのパスを返す。既存のファイルは上書きしない。
    pub fn save(&self, markdown: &str, now: SystemTime) -> io::Result<PathBuf> {
        fs::create_dir_all(&self.dir)?;
        let stem = file_stem(now);
        for n in 1.. {
            let name = if n == 1 {
                format!("{stem}.md")
            } else {
                format!("{stem}-{n}.md")
            };
            let path = self.dir.join(name);
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    file.write_all(markdown.as_bytes())?;
                    return Ok(path);
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        unreachable!()
    }
}

/// `conditions-20261106-074500Z` の形（UTC）。日付の crate を足さずに済ませるため UTC にしている。
fn file_stem(now: SystemTime) -> String {
    let secs = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "conditions-{y:04}{m:02}{d:02}-{:02}{:02}{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// 1970-01-01 からの日数を年月日にする（Howard Hinnant の civil_from_days）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn env_overrides_the_default_place() {
        assert_eq!(
            data_dir_from(Some("/x/data".into()), Some("/home/u".into())),
            PathBuf::from("/x/data")
        );
        assert_eq!(
            data_dir_from(None, Some("/home/u".into())),
            PathBuf::from("/home/u/Library/Application Support/mytool")
        );
        assert_eq!(
            data_dir_from(Some("".into()), Some("/home/u".into())),
            PathBuf::from("/home/u/Library/Application Support/mytool")
        );
    }

    #[test]
    fn names_files_by_utc_time() {
        assert_eq!(file_stem(at(0)), "conditions-19700101-000000Z");
        // 2026-11-06 07:45:30 UTC
        assert_eq!(file_stem(at(1_793_951_130)), "conditions-20261106-074530Z");
        // うるう日
        assert_eq!(file_stem(at(1_709_164_800)), "conditions-20240229-000000Z");
    }

    #[test]
    fn saves_under_requests_and_returns_the_path() {
        let tmp = tempfile::tempdir().unwrap();
        let store = RequestStore::new(tmp.path());

        let path = store.save("# 今回の条件: x\n", at(0)).unwrap();

        assert_eq!(path.parent().unwrap(), tmp.path().join("requests"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "# 今回の条件: x\n");
    }

    #[test]
    fn does_not_overwrite_a_file_saved_in_the_same_second() {
        let tmp = tempfile::tempdir().unwrap();
        let store = RequestStore::new(tmp.path());

        let first = store.save("一", at(0)).unwrap();
        let second = store.save("二", at(0)).unwrap();

        assert_ne!(first, second);
        assert_eq!(fs::read_to_string(first).unwrap(), "一");
        assert_eq!(fs::read_to_string(second).unwrap(), "二");
    }
}
