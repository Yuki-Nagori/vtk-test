//! Cargo 质量命令分发；具体构建、检查和报告由各职责模块承接。

mod build;
mod command;
mod format;
mod lint;
mod paths;

use std::error::Error;
use std::process::ExitCode;

fn main() -> ExitCode {
    let result = dispatch(std::env::args().skip(1));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("panta-tests: {error}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch(mut arguments: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    match arguments.next().as_deref() {
        Some("quality") => quality(),
        Some("format") => {
            let mut rest: Vec<String> = arguments.collect();
            let check = take_check_flag(&mut rest);
            format::format_all(check, &rest)
        }
        Some("lint") => {
            let mut rest: Vec<String> = arguments.collect();
            let check = take_check_flag(&mut rest);
            let tool = rest.first().map(String::as_str);
            let tool_args = rest.get(1..).unwrap_or_default();
            lint::lint(tool, check, tool_args)
        }
        Some(command) => Err(format!("未知命令 '{command}'；可用：quality、lint、format").into()),
        None => Err("缺少命令；可用：quality、lint、format".into()),
    }
}

/// 从剩余参数摘除 `--check`（位置不限）。缺省行为是就地修复（写文件），
/// `--check` 只验证不改动；CI 一律带 `--check`。
fn take_check_flag(arguments: &mut Vec<String>) -> bool {
    let check = arguments.iter().any(|argument| argument == "--check");
    arguments.retain(|argument| argument != "--check");
    check
}

fn quality() -> Result<(), Box<dyn Error>> {
    format::format_all(true, &[])?;
    lint::lint(None, true, &[])?;
    command::cargo("test", ["--locked", "--workspace"])
}

#[cfg(test)]
mod tests {
    use super::take_check_flag;

    #[test]
    fn check_flag_is_removed_without_reordering_arguments() {
        let mut arguments = vec![
            "--check".to_owned(),
            "clippy".to_owned(),
            "--check".to_owned(),
        ];
        assert!(take_check_flag(&mut arguments));
        assert_eq!(arguments, ["clippy"]);
        assert!(!take_check_flag(&mut arguments));
        assert_eq!(arguments, ["clippy"]);
    }
}
