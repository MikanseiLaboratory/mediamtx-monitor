//! 依存を増やさないための最小限の引数パーサ。
//!
//! `--smoke-exit-after-ms <ms>`: 起動確認用に指定時間で自動終了する。

/// 解釈済みの引数。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Args {
    /// 指定ミリ秒後に自分で終了する（E2E・起動確認用）。
    pub smoke_exit_after_ms: Option<u64>,
    pub help: bool,
    pub version: bool,
}

impl Args {
    pub const USAGE: &'static str = "\
mediamtx-monitor — MediaMTX 死活監視 GUI

使い方:
  mediamtx-monitor [OPTIONS]

オプション:
  --smoke-exit-after-ms <MS>  指定ミリ秒後に終了する（起動確認用）
  -h, --help                  このヘルプを表示する
  -V, --version               バージョンを表示する
";

    pub fn parse_from(raw: Vec<String>) -> Self {
        let mut out = Args::default();
        let mut iter = raw.into_iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "-h" | "--help" => out.help = true,
                "-V" | "--version" => out.version = true,
                "--smoke-exit-after-ms" => {
                    if let Some(v) = iter.next() {
                        out.smoke_exit_after_ms = v.trim().parse::<u64>().ok();
                    }
                }
                other => {
                    if let Some(v) = other.strip_prefix("--smoke-exit-after-ms=") {
                        out.smoke_exit_after_ms = v.trim().parse::<u64>().ok();
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(items: &[&str]) -> Args {
        Args::parse_from(items.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn parses_smoke_flag() {
        assert_eq!(parse(&[]).smoke_exit_after_ms, None);
        assert_eq!(
            parse(&["--smoke-exit-after-ms", "1500"]).smoke_exit_after_ms,
            Some(1500)
        );
        assert_eq!(
            parse(&["--smoke-exit-after-ms=2500"]).smoke_exit_after_ms,
            Some(2500)
        );
        assert_eq!(
            parse(&["--smoke-exit-after-ms", "oops"]).smoke_exit_after_ms,
            None
        );
    }

    #[test]
    fn parses_help_and_version() {
        assert!(parse(&["--help"]).help);
        assert!(parse(&["-h"]).help);
        assert!(parse(&["-V"]).version);
        assert!(!parse(&["--unknown"]).help);
    }
}
