//! OpenFOAM 算例的确定性快照：一个算例由许多文件组成，导入链路的快照 / 摘要 /
//! 源变更检查都以单个字节串为单位，因此把选中的文件按固定顺序拼成一个包。
//!
//! 布局：`PANTA-FOAM-CASE 1\n`，u32 条目数，然后每条 u32 路径长度 + UTF-8 相对路径
//! （`/` 分隔）+ u64 内容长度 + 原始内容（`.gz` 保持压缩形态）。全部小端。
//! 路径 `skipped/<region>/<field>` 的条目内容为场类别名，记录不支持的体场。

use super::super::{FormatError, FormatResult};
use std::path::{Path, PathBuf};

pub const MAGIC: &[u8] = b"PANTA-FOAM-CASE 1\n";

/// 算例中被收入快照的文件（相对算例根的路径与绝对路径）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseFile {
    pub relative: String,
    pub path: PathBuf,
}

/// 发现阶段的结果：要读取的文件（已排序）、跳过的体场、选中的时间目录与日志说明。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CasePlan {
    pub root: PathBuf,
    pub files: Vec<CaseFile>,
    pub skipped: Vec<(String, String)>,
    pub time: Option<String>,
    pub regions: Vec<String>,
}

const MESH_FILES: [&str; 5] = ["points", "faces", "owner", "neighbour", "boundary"];

/// 算例根：`.foam` / `.openfoam` 标记文件的父目录，或 `system/controlDict` 的上上级。
pub fn case_root(selected: &Path) -> Option<PathBuf> {
    let parent = selected.parent()?;
    if selected
        .file_name()
        .is_some_and(|name| name == "controlDict")
    {
        return parent.parent().map(Path::to_path_buf);
    }
    Some(parent.to_path_buf())
}

fn sorted_entries(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| Some((entry.file_name().into_string().ok()?, entry.path())))
        .collect();
    entries.sort();
    entries
}

/// 同名文件优先未压缩版本，其次 `.gz`（与 OpenFOAM 读取顺序一致）。
fn mesh_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let plain = dir.join(name);
    if plain.is_file() {
        return Some(plain);
    }
    let gz = dir.join(format!("{name}.gz"));
    gz.is_file().then_some(gz)
}

fn is_time_name(name: &str) -> Option<f64> {
    if name.starts_with('+') || name.starts_with('-') {
        return None;
    }
    name.parse::<f64>().ok().filter(|value| value.is_finite())
}

/// 读取文件开头（`.gz` 先解压）判断场类别；非 FoamFile 返回 `None`。
fn peek_class(path: &Path) -> Option<String> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let mut head = Vec::new();
    let limit = 8 * 1024;
    if path.extension().is_some_and(|ext| ext == "gz") {
        flate2::read::GzDecoder::new(file)
            .take(limit)
            .read_to_end(&mut head)
            .ok()?;
    } else {
        file.take(limit).read_to_end(&mut head).ok()?;
    }
    let lexer = super::lexer::Lexer::new(&head).ok()?;
    (!lexer.header.class.is_empty()).then(|| lexer.header.class.clone())
}

/// 列出算例要读取的文件：全部区域的 polyMesh，与最新时间目录下的体标量 / 矢量场。
/// 存在 `constant/<name>/polyMesh` 时按多区域导入（忽略默认区域），否则用 `constant/polyMesh`。
pub fn plan_case(selected: &Path) -> Result<CasePlan, String> {
    let root = case_root(selected).ok_or_else(|| "cannot locate the case directory".to_owned())?;
    let constant = root.join("constant");
    let mut regions: Vec<String> = sorted_entries(&constant)
        .into_iter()
        .filter(|(name, path)| {
            name != "polyMesh" && mesh_file(&path.join("polyMesh"), "faces").is_some()
        })
        .map(|(name, _)| name)
        .collect();
    if regions.is_empty() {
        if mesh_file(&constant.join("polyMesh"), "faces").is_none() {
            let decomposed = root.join("processor0").is_dir();
            return Err(if decomposed {
                "decomposed case without constant/polyMesh; run reconstructPar first".to_owned()
            } else {
                format!("{} has no constant/polyMesh", root.display())
            });
        }
        regions.push(String::new());
    }
    let region_dir = |base: &Path, region: &str| {
        if region.is_empty() {
            base.to_path_buf()
        } else {
            base.join(region)
        }
    };
    let mut plan = CasePlan {
        root: root.clone(),
        regions: regions.clone(),
        ..CasePlan::default()
    };
    for region in &regions {
        let mesh_dir = region_dir(&constant, region).join("polyMesh");
        let prefix = relative_prefix("constant", region);
        for name in MESH_FILES {
            let Some(path) = mesh_file(&mesh_dir, name) else {
                if name == "neighbour" {
                    continue;
                }
                return Err(format!("{} is missing {name}", mesh_dir.display()));
            };
            let file_name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            plan.files.push(CaseFile {
                relative: format!("{prefix}polyMesh/{file_name}"),
                path,
            });
        }
    }
    let time = sorted_entries(&root)
        .into_iter()
        .filter(|(_, path)| path.is_dir())
        .filter_map(|(name, _)| Some((is_time_name(&name)?, name)))
        .max_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, name)| name);
    if let Some(time) = &time {
        for region in &regions {
            let dir = region_dir(&root.join(time), region);
            let prefix = relative_prefix(time, region);
            for (name, path) in sorted_entries(&dir) {
                if !path.is_file() {
                    continue;
                }
                let Some(class) = peek_class(&path) else {
                    continue;
                };
                let field = name.strip_suffix(".gz").unwrap_or(&name).to_owned();
                match class.as_str() {
                    "volScalarField" | "volVectorField" => plan.files.push(CaseFile {
                        relative: format!("{prefix}{name}"),
                        path,
                    }),
                    other if other.starts_with("vol") && other.ends_with("Field") => {
                        plan.skipped.push((
                            format!("{}{field}", relative_prefix("skipped", region)),
                            other.to_owned(),
                        ))
                    }
                    _ => {}
                }
            }
        }
    }
    plan.time = time;
    Ok(plan)
}

fn relative_prefix(base: &str, region: &str) -> String {
    if region.is_empty() {
        format!("{base}/")
    } else {
        format!("{base}/{region}/")
    }
}

/// 编码一个条目头（路径与内容长度）；内容由调用方逐块追加。
pub fn push_entry_header(bytes: &mut Vec<u8>, relative: &str, length: u64) -> Result<(), String> {
    let path_length = u32::try_from(relative.len()).map_err(|_| "path too long".to_owned())?;
    bytes.extend(path_length.to_le_bytes());
    bytes.extend(relative.as_bytes());
    bytes.extend(length.to_le_bytes());
    Ok(())
}

/// 解码后的条目（内容为原始字节，`.gz` 未解压）。
pub struct Entry<'a> {
    pub relative: &'a str,
    pub bytes: &'a [u8],
}

pub fn decode(bytes: &[u8]) -> FormatResult<Vec<Entry<'_>>> {
    struct Reader<'a> {
        rest: &'a [u8],
    }
    impl<'a> Reader<'a> {
        fn take(&mut self, length: usize) -> FormatResult<&'a [u8]> {
            if self.rest.len() < length {
                return Err(FormatError::new("malformed OpenFOAM case snapshot"));
            }
            let (head, tail) = self.rest.split_at(length);
            self.rest = tail;
            Ok(head)
        }
        fn word<const N: usize>(&mut self) -> FormatResult<[u8; N]> {
            let mut word = [0u8; N];
            word.copy_from_slice(self.take(N)?);
            Ok(word)
        }
        fn length(&mut self, wide: bool) -> FormatResult<usize> {
            let value = if wide {
                u64::from_le_bytes(self.word()?)
            } else {
                u64::from(u32::from_le_bytes(self.word()?))
            };
            usize::try_from(value).map_err(|_| FormatError::new("malformed OpenFOAM case snapshot"))
        }
    }
    let rest = bytes
        .strip_prefix(MAGIC)
        .ok_or_else(|| FormatError::new("malformed OpenFOAM case snapshot"))?;
    let mut reader = Reader { rest };
    let count = reader.length(false)?;
    let mut entries = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        let path_length = reader.length(false)?;
        let relative = std::str::from_utf8(reader.take(path_length)?)
            .map_err(|_| FormatError::new("malformed OpenFOAM case snapshot"))?;
        let length = reader.length(true)?;
        entries.push(Entry {
            relative,
            bytes: reader.take(length)?,
        });
    }
    if !reader.rest.is_empty() {
        return Err(FormatError::new("malformed OpenFOAM case snapshot"));
    }
    Ok(entries)
}
