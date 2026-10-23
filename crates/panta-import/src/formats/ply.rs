//! Stanford PLY（ascii / binary_little_endian / binary_big_endian）：读取 vertex 的
//! x、y、z 与 face 的 vertex_indices（或 vertex_index）列表，其它元素和属性按声明跳过。

use panta_mesh::SurfaceMesh;

use super::text::{Cursor, Scalar};
use super::{FormatError, FormatResult, IndexedPoints, TriangleSink, checked_point, finish_mesh};

pub fn parse_ply(bytes: &[u8]) -> FormatResult<SurfaceMesh> {
    let mut mesh = SurfaceMesh {
        triangles: Vec::new(),
    };
    read_ply(bytes, &mut mesh)?;
    finish_mesh(mesh)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Ascii,
    Binary { little_endian: bool },
}

enum Property {
    Scalar(String, Scalar),
    List(String, Scalar, Scalar),
}

struct Element {
    name: String,
    count: u64,
    properties: Vec<Property>,
}

fn scalar(name: &str) -> FormatResult<Scalar> {
    Ok(match name {
        "char" | "int8" => Scalar::I8,
        "uchar" | "uint8" => Scalar::U8,
        "short" | "int16" => Scalar::I16,
        "ushort" | "uint16" => Scalar::U16,
        "int" | "int32" => Scalar::I32,
        "uint" | "uint32" => Scalar::U32,
        "float" | "float32" => Scalar::F32,
        "double" | "float64" => Scalar::F64,
        other => return Err(FormatError::new(format!("unknown PLY type {other}"))),
    })
}

pub(crate) fn read_ply(bytes: &[u8], sink: &mut impl TriangleSink) -> FormatResult<()> {
    let mut cursor = Cursor::new(bytes);
    if cursor.line()?.trim() != "ply" {
        return Err(FormatError::new("missing PLY magic"));
    }
    let mut encoding = None;
    let mut elements: Vec<Element> = Vec::new();
    loop {
        let line = cursor.line()?;
        let fields: Vec<&str> = line.split_whitespace().collect();
        match fields.as_slice() {
            ["format", "ascii", _] => encoding = Some(Encoding::Ascii),
            ["format", "binary_little_endian", _] => {
                encoding = Some(Encoding::Binary {
                    little_endian: true,
                });
            }
            ["format", "binary_big_endian", _] => {
                encoding = Some(Encoding::Binary {
                    little_endian: false,
                });
            }
            ["element", name, count] => elements.push(Element {
                name: (*name).to_owned(),
                count: count
                    .parse()
                    .map_err(|_| FormatError::new("invalid PLY element count"))?,
                properties: Vec::new(),
            }),
            ["property", "list", count, item, name] => elements
                .last_mut()
                .ok_or_else(|| FormatError::new("PLY property before element"))?
                .properties
                .push(Property::List(
                    (*name).to_owned(),
                    scalar(count)?,
                    scalar(item)?,
                )),
            ["property", kind, name] => elements
                .last_mut()
                .ok_or_else(|| FormatError::new("PLY property before element"))?
                .properties
                .push(Property::Scalar((*name).to_owned(), scalar(kind)?)),
            ["end_header"] => break,
            _ => {}
        }
    }
    let encoding = encoding.ok_or_else(|| FormatError::new("missing PLY format"))?;
    let mut reader = ValueReader { cursor, encoding };
    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut faces: Vec<Vec<u64>> = Vec::new();
    for element in &elements {
        let axes: Vec<Option<usize>> = element
            .properties
            .iter()
            .map(|property| match property {
                Property::Scalar(name, _) => ["x", "y", "z"].iter().position(|axis| axis == name),
                Property::List(..) => None,
            })
            .collect();
        for _ in 0..element.count {
            let mut point = [0.0; 3];
            for (property, axis) in element.properties.iter().zip(&axes) {
                match property {
                    Property::Scalar(_, kind) => {
                        let value = reader.float(*kind)?;
                        if let Some(axis) = axis {
                            point[*axis] = value;
                        }
                    }
                    Property::List(name, count_kind, item_kind) => {
                        let count = reader.index(*count_kind)?;
                        let mut items = Vec::new();
                        for _ in 0..count {
                            items.push(reader.index(*item_kind)?);
                        }
                        if element.name == "face"
                            && (name == "vertex_indices" || name == "vertex_index")
                        {
                            faces.push(items);
                        }
                    }
                }
            }
            if element.name == "vertex" {
                points.push(checked_point(point)?);
            }
        }
    }
    let indexed = IndexedPoints { points: &points };
    for face in &faces {
        indexed.polygon(face, sink)?;
    }
    Ok(())
}

struct ValueReader<'a> {
    cursor: Cursor<'a>,
    encoding: Encoding,
}

impl ValueReader<'_> {
    fn float(&mut self, kind: Scalar) -> FormatResult<f64> {
        match self.encoding {
            Encoding::Ascii => self.cursor.parse(),
            Encoding::Binary { little_endian } => {
                Ok(kind.decode_f64(self.cursor.raw(kind.size())?, little_endian))
            }
        }
    }

    fn index(&mut self, kind: Scalar) -> FormatResult<u64> {
        match self.encoding {
            Encoding::Ascii => self.cursor.parse(),
            Encoding::Binary { little_endian } => {
                kind.decode_index(self.cursor.raw(kind.size())?, little_endian)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_ply;

    #[test]
    fn ascii_quad_with_extra_properties_is_triangulated() -> Result<(), super::FormatError> {
        let mesh = parse_ply(
            b"ply\nformat ascii 1.0\ncomment test\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0 1\n1 0 0 1\n1 1 0 1\n0 1 0 1\n4 0 1 2 3\n",
        )?;
        assert_eq!(mesh.triangles.len(), 2);
        Ok(())
    }

    #[test]
    fn truncated_binary_body_is_rejected() {
        let mut bytes = b"ply\nformat binary_little_endian 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nend_header\n".to_vec();
        bytes.extend_from_slice(&[0; 20]);
        assert!(parse_ply(&bytes).is_err());
    }
}
