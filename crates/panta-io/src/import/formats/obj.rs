//! Wavefront OBJ：读取 `v` 顶点与 `f` 多边形面（支持 `v/vt/vn` 与负索引），忽略材质、
//! 纹理、法向、线和分组。

use panta_mesh::SurfaceMesh;

use super::{FormatError, FormatResult, IndexedPoints, TriangleSink, checked_point, finish_mesh};

pub fn parse_obj(bytes: &[u8]) -> FormatResult<SurfaceMesh> {
    let mut mesh = SurfaceMesh {
        triangles: Vec::new(),
    };
    read_obj(bytes, &mut mesh)?;
    finish_mesh(mesh)
}

/// OBJ 面可引用此前出现的任意顶点，因此先收集全部顶点再输出面。
pub(crate) fn read_obj(bytes: &[u8], sink: &mut impl TriangleSink) -> FormatResult<()> {
    let text = std::str::from_utf8(bytes).map_err(|_| FormatError::new("OBJ is not UTF-8"))?;
    let mut points = Vec::new();
    let mut faces: Vec<Vec<u64>> = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let mut fields = line.split_whitespace();
        match fields.next() {
            Some("v") => {
                let mut point = [0.0; 3];
                for coordinate in &mut point {
                    *coordinate = fields
                        .next()
                        .and_then(|field| field.parse::<f64>().ok())
                        .ok_or_else(|| {
                            FormatError::new(format!("line {}: invalid vertex", number + 1))
                        })?;
                }
                points.push(checked_point(point)?);
            }
            Some("f") => {
                let face = fields
                    .map(|field| resolve_index(field, points.len()))
                    .collect::<Option<Vec<u64>>>()
                    .ok_or_else(|| {
                        FormatError::new(format!("line {}: invalid face index", number + 1))
                    })?;
                faces.push(face);
            }
            _ => {}
        }
    }
    let indexed = IndexedPoints { points: &points };
    for face in &faces {
        indexed.polygon(face, sink)?;
    }
    Ok(())
}

/// 正索引从 1 开始；负索引相对当前已读顶点数，按出现位置解析。
fn resolve_index(field: &str, defined: usize) -> Option<u64> {
    let index: i64 = field.split('/').next()?.parse().ok()?;
    let defined = i64::try_from(defined).ok()?;
    let resolved = if index > 0 {
        index - 1
    } else if index < 0 {
        defined + index
    } else {
        return None;
    };
    u64::try_from(resolved).ok()
}

#[cfg(test)]
mod tests {
    use super::parse_obj;

    #[test]
    fn quads_are_fan_triangulated_and_relative_indices_resolve() -> Result<(), super::FormatError> {
        let mesh = parse_obj(
            b"# square\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nvn 0 0 1\nf 1//1 2//1 3//1 4//1\nf -4 -3 -2\n",
        )?;
        assert_eq!(mesh.triangles.len(), 3);
        assert_eq!(mesh.triangles[2][2], [1.0, 1.0, 0.0]);
        Ok(())
    }

    #[test]
    fn out_of_range_and_zero_indices_are_rejected() {
        assert!(parse_obj(b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 4\n").is_err());
        assert!(parse_obj(b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 0 1 2\n").is_err());
        assert!(parse_obj(b"v 0 0 0\n").is_err());
    }
}
