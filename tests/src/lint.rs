//! Rust Clippy 与 QML 静态检查入口。
use std::error::Error;

use crate::build::{build_launcher, run_qmllint};
use crate::command::cargo;

pub(super) fn lint(
    tool: Option<&str>,
    check: bool,
    arguments: &[String],
) -> Result<(), Box<dyn Error>> {
    if !arguments.is_empty() {
        return Err("lint 仅接受 clippy、qmllint 及 --check".into());
    }
    match tool {
        None => {
            lint(Some("clippy"), check, &[])?;
            lint(Some("qmllint"), check, &[])
        }
        Some("clippy") if check => cargo(
            "clippy",
            [
                "--locked",
                "--workspace",
                "--all-targets",
                "--target-dir",
                "--",
                "-D",
                "warnings",
            ],
        ),
        // 修复模式先应用机器可修复建议，再落回检查模式确认剩余问题。
        Some("clippy") => {
            cargo(
                "clippy",
                [
                    "--fix",
                    "--locked",
                    "--workspace",
                    "--all-targets",
                    "--allow-dirty",
                ],
            )?;
            lint(Some("clippy"), true, &[])
        }
        Some("qmllint") => {
            build_launcher()?;
            run_qmllint()
        }
        Some(other) => Err(format!(
            "未知 lint 工具 '{other}'；可用：clippy、qmllint；--check 只检查不修复"
        )
        .into()),
    }
}
