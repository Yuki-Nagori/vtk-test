//! 旧版 VTK（`.vtk`）ASCII / BINARY 的 POLYDATA 与 UNSTRUCTURED_GRID；同时支持 5.1 版
//! OFFSETS / CONNECTIVITY 单元布局和 5.1 之前的计数前缀布局。表面导入忽略属性数据；
//! 体网格导入读取 POINT_DATA / CELL_DATA 中的 SCALARS、VECTORS、NORMALS 与 FIELD 数组（task 029）。

use panta_mesh::SurfaceMesh;

use super::cells::CellSurface;
use super::text::{Cursor, Scalar, decode_f64_array, decode_index_array, points_from_flat};
use super::{
    CellList, FormatError, FormatResult, IndexedPoints, RawGrid, TriangleSink, finish_mesh,
};

pub fn parse_vtk_legacy(bytes: &[u8]) -> FormatResult<SurfaceMesh> {
    let mut mesh = SurfaceMesh {
        triangles: Vec::new(),
    };
    read_vtk_legacy(bytes, &mut mesh, None)?;
    finish_mesh(mesh)
}

const TRIANGLE_STRIP: u8 = 6;
const POLYGON: u8 = 7;

fn scalar(name: &str) -> FormatResult<Scalar> {
    Ok(match name.to_ascii_lowercase().as_str() {
        "char" | "vtktypeint8" => Scalar::I8,
        "unsigned_char" | "bit" | "vtktypeuint8" => Scalar::U8,
        "short" | "vtktypeint16" => Scalar::I16,
        "unsigned_short" | "vtktypeuint16" => Scalar::U16,
        "int" | "vtktypeint32" => Scalar::I32,
        "unsigned_int" | "vtktypeuint32" => Scalar::U32,
        "long" | "vtkidtype" | "vtktypeint64" => Scalar::I64,
        "unsigned_long" | "vtktypeuint64" => Scalar::U64,
        "float" => Scalar::F32,
        "double" => Scalar::F64,
        other => return Err(FormatError::new(format!("unknown VTK data type {other}"))),
    })
}

struct Reader<'a> {
    cursor: Cursor<'a>,
    binary: bool,
}

impl Reader<'_> {
    fn floats(&mut self, count: usize, kind: Scalar) -> FormatResult<Vec<f64>> {
        if self.binary {
            self.cursor.finish_line()?;
            let length = count
                .checked_mul(kind.size())
                .ok_or_else(|| FormatError::new("array size overflow"))?;
            decode_f64_array(self.cursor.raw(length)?, kind, count, false)
        } else {
            (0..count).map(|_| self.cursor.parse::<f64>()).collect()
        }
    }

    fn indices(&mut self, count: usize, kind: Scalar) -> FormatResult<Vec<u64>> {
        if self.binary {
            self.cursor.finish_line()?;
            let length = count
                .checked_mul(kind.size())
                .ok_or_else(|| FormatError::new("array size overflow"))?;
            decode_index_array(self.cursor.raw(length)?, kind, count, false)
        } else {
            (0..count).map(|_| self.cursor.parse::<u64>()).collect()
        }
    }

    fn count(&mut self) -> FormatResult<usize> {
        self.cursor.parse()
    }

    /// 读取一个单元数组，统一转为扁平单元表。
    fn cells(&mut self, modern: bool) -> FormatResult<CellList> {
        let first = self.count()?;
        let second = self.count()?;
        if modern {
            let (offsets, connectivity) = (first, second);
            expect_keyword(&mut self.cursor, "OFFSETS")?;
            let kind = scalar(self.cursor.token()?)?;
            let offsets = self.indices(offsets, kind)?;
            expect_keyword(&mut self.cursor, "CONNECTIVITY")?;
            let kind = scalar(self.cursor.token()?)?;
            let connectivity = self.indices(connectivity, kind)?;
            if offsets.first().is_some_and(|&start| start != 0) {
                return Err(FormatError::new("cell offsets must start at 0"));
            }
            CellList::from_ends(connectivity, offsets.get(1..).unwrap_or_default())
        } else {
            let (cells, size) = (first, second);
            let flat = self.indices(size, Scalar::I32)?;
            let mut connectivity = Vec::with_capacity(size.saturating_sub(cells));
            let mut ends = Vec::with_capacity(cells);
            let mut position = 0usize;
            for _ in 0..cells {
                let count = flat
                    .get(position)
                    .and_then(|&count| usize::try_from(count).ok())
                    .ok_or_else(|| FormatError::new("truncated cell list"))?;
                let nodes = flat
                    .get(position + 1..position + 1 + count)
                    .ok_or_else(|| FormatError::new("truncated cell list"))?;
                connectivity.extend_from_slice(nodes);
                ends.push(connectivity.len());
                position += 1 + count;
            }
            Ok(CellList { connectivity, ends })
        }
    }
}

fn expect_keyword(cursor: &mut Cursor<'_>, keyword: &str) -> FormatResult<()> {
    let token = cursor.token()?;
    if token.eq_ignore_ascii_case(keyword) {
        Ok(())
    } else {
        Err(FormatError::new(format!(
            "expected {keyword}, found {token}"
        )))
    }
}

/// 旧版 VTK UNSTRUCTURED_GRID 线性四面体体网格的原始节点与单元（task 021）。
pub fn parse_vtk_legacy_grid(bytes: &[u8]) -> FormatResult<RawGrid> {
    let mut grid = RawGrid::default();
    read_vtk_legacy(
        bytes,
        &mut SurfaceMesh {
            triangles: Vec::new(),
        },
        Some(&mut grid),
    )?;
    Ok(grid)
}

pub(crate) fn read_vtk_legacy(
    bytes: &[u8],
    sink: &mut impl TriangleSink,
    grid: Option<&mut RawGrid>,
) -> FormatResult<()> {
    let mut cursor = Cursor::new(bytes);
    let header = cursor.line()?;
    let version = header
        .strip_prefix("# vtk DataFile Version ")
        .ok_or_else(|| FormatError::new("missing VTK legacy header"))?;
    let major: u32 = version
        .trim()
        .split('.')
        .next()
        .and_then(|major| major.parse().ok())
        .ok_or_else(|| FormatError::new("invalid VTK legacy version"))?;
    let modern = major >= 5;
    cursor.line()?;
    let binary = match cursor.token()?.to_ascii_uppercase().as_str() {
        "ASCII" => false,
        "BINARY" => true,
        other => return Err(FormatError::new(format!("unknown VTK encoding {other}"))),
    };
    expect_keyword(&mut cursor, "DATASET")?;
    let dataset = cursor.token()?.to_ascii_uppercase();
    if dataset != "POLYDATA" && dataset != "UNSTRUCTURED_GRID" {
        return Err(FormatError::new(format!(
            "unsupported VTK dataset {dataset}"
        )));
    }
    let mut reader = Reader { cursor, binary };
    let mut points: Option<Vec<[f64; 3]>> = None;
    let mut polygons: Vec<(u8, CellList)> = Vec::new();
    let mut cells: Option<CellList> = None;
    let mut cell_types: Option<Vec<u64>> = None;
    let mut attributes: Option<String> = None;
    while !reader.cursor.is_at_end() {
        let keyword = reader.cursor.token()?.to_ascii_uppercase();
        match keyword.as_str() {
            "POINTS" => {
                let count = reader.count()?;
                let kind = scalar(reader.cursor.token()?)?;
                let length = count
                    .checked_mul(3)
                    .ok_or_else(|| FormatError::new("point count overflow"))?;
                points = Some(points_from_flat(&reader.floats(length, kind)?)?);
            }
            "METADATA" => skip_metadata(&mut reader.cursor)?,
            "VERTICES" | "LINES" => {
                reader.cells(modern)?;
            }
            "POLYGONS" => polygons.push((POLYGON, reader.cells(modern)?)),
            "TRIANGLE_STRIPS" => polygons.push((TRIANGLE_STRIP, reader.cells(modern)?)),
            "CELLS" => cells = Some(reader.cells(modern)?),
            "CELL_TYPES" => {
                let count = reader.count()?;
                cell_types = Some(reader.indices(count, Scalar::I32)?);
            }
            "POINT_DATA" | "CELL_DATA" | "FIELD" => {
                attributes = Some(keyword);
                break;
            }
            other => return Err(FormatError::new(format!("unsupported VTK section {other}"))),
        }
    }
    let points = points.ok_or_else(|| FormatError::new("missing POINTS"))?;
    if let Some(grid) = grid {
        if dataset != "UNSTRUCTURED_GRID" {
            return Err(FormatError::new("volume import requires UNSTRUCTURED_GRID"));
        }
        let cells = cells.ok_or_else(|| FormatError::new("missing CELLS"))?;
        let types = cell_types.ok_or_else(|| FormatError::new("missing CELL_TYPES"))?;
        *grid = RawGrid::new(points, cells, types, None)?;
        if let Some(keyword) = attributes {
            read_attributes(&mut reader, &keyword, grid);
        }
        return Ok(());
    }
    let indexed = IndexedPoints { points: &points };
    for (kind, list) in &polygons {
        for nodes in list.iter() {
            if *kind == POLYGON {
                indexed.polygon(nodes, sink)?;
            } else {
                indexed.strip(nodes, sink)?;
            }
        }
    }
    if let Some(cells) = cells {
        let types = cell_types.ok_or_else(|| FormatError::new("missing CELL_TYPES"))?;
        if types.len() != cells.len() {
            return Err(FormatError::new("CELL_TYPES count mismatch"));
        }
        let mut surface = CellSurface::new(&points);
        for (kind, nodes) in types.iter().zip(cells.iter()) {
            let kind = u8::try_from(*kind).map_err(|_| FormatError::new("invalid cell type"))?;
            surface.add(kind, nodes, sink)?;
        }
        surface.finish(sink)?;
    }
    Ok(())
}

/// 读取属性段直到文件末尾。属性段只影响结果场：遇到无法解析或不认识的内容时停止，
/// 已读出的数组保留，其余记入 `skipped_fields`，网格导入照常完成。
fn read_attributes(reader: &mut Reader<'_>, first: &str, grid: &mut RawGrid) {
    let mut keyword = first.to_owned();
    let mut association = None;
    loop {
        match read_attribute(reader, &keyword, &mut association, grid) {
            Ok(()) => {}
            Err(error) => {
                grid.skipped_fields.push(format!(
                    "attribute section {keyword}: {error}; remaining attributes ignored"
                ));
                return;
            }
        }
        if reader.cursor.is_at_end() {
            return;
        }
        match reader.cursor.token() {
            Ok(token) => keyword = token.to_ascii_uppercase(),
            Err(_) => return,
        }
    }
}

fn read_attribute(
    reader: &mut Reader<'_>,
    keyword: &str,
    association: &mut Option<panta_mesh::FieldAssociation>,
    grid: &mut RawGrid,
) -> FormatResult<()> {
    use panta_mesh::FieldAssociation;
    match keyword {
        "POINT_DATA" | "CELL_DATA" => {
            let (kind, expected) = if keyword == "POINT_DATA" {
                (FieldAssociation::Node, grid.points.len())
            } else {
                (FieldAssociation::Cell, grid.cell_count())
            };
            if reader.count()? != expected {
                return Err(FormatError::new(format!(
                    "{keyword} count does not match the grid"
                )));
            }
            *association = Some(kind);
        }
        "METADATA" => skip_metadata(&mut reader.cursor)?,
        "SCALARS" => {
            let current = association.ok_or_else(|| FormatError::new("SCALARS outside data"))?;
            let name = decode_name(reader.cursor.token()?);
            let kind = scalar(reader.cursor.token()?)?;
            let line = reader.cursor.line()?.trim();
            let components = if line.is_empty() {
                1
            } else {
                line.parse::<usize>()
                    .ok()
                    .filter(|count| (1..=4).contains(count))
                    .ok_or_else(|| FormatError::new("invalid SCALARS component count"))?
            };
            if reader
                .cursor
                .peek_token()
                .is_some_and(|token| token.eq_ignore_ascii_case("LOOKUP_TABLE"))
            {
                reader.cursor.token()?;
                reader.cursor.token()?;
            } else if reader.binary {
                return Err(FormatError::new("SCALARS without LOOKUP_TABLE"));
            }
            let count = tuple_values(grid, current, components)?;
            let values = reader.floats(count, kind)?;
            grid.offer_field(&name, current, components, Ok(values));
        }
        "VECTORS" | "NORMALS" => {
            let current = association.ok_or_else(|| FormatError::new("VECTORS outside data"))?;
            let name = decode_name(reader.cursor.token()?);
            let kind = scalar(reader.cursor.token()?)?;
            let values = reader.floats(tuple_values(grid, current, 3)?, kind)?;
            grid.offer_field(&name, current, 3, Ok(values));
        }
        "TENSORS" | "TENSORS6" | "TEXTURE_COORDINATES" | "COLOR_SCALARS" => {
            let current = association.ok_or_else(|| FormatError::new("array outside data"))?;
            let name = decode_name(reader.cursor.token()?);
            let (components, kind) = match keyword {
                "TENSORS" => (9, scalar(reader.cursor.token()?)?),
                "TENSORS6" => (6, scalar(reader.cursor.token()?)?),
                "TEXTURE_COORDINATES" => {
                    let dimension = reader.count()?;
                    (dimension, scalar(reader.cursor.token()?)?)
                }
                // COLOR_SCALARS 二进制为 unsigned char，ASCII 为 0–1 浮点。
                _ => (reader.count()?, Scalar::U8),
            };
            let count = tuple_values(grid, current, components)?;
            reader.floats(count, kind)?;
            grid.skipped_fields.push(format!(
                "{name} ({}): {keyword} is not supported",
                current.as_str()
            ));
        }
        "LOOKUP_TABLE" => {
            reader.cursor.token()?;
            let size = reader.count()?;
            let count = size
                .checked_mul(4)
                .ok_or_else(|| FormatError::new("lookup table size overflow"))?;
            reader.floats(count, Scalar::U8)?;
        }
        "FIELD" => {
            let current = association.ok_or_else(|| FormatError::new("FIELD outside data"))?;
            reader.cursor.token()?;
            let arrays = reader.count()?;
            for _ in 0..arrays {
                let name = decode_name(reader.cursor.token()?);
                let components = reader.count()?;
                let tuples = reader.count()?;
                let kind = scalar(reader.cursor.token()?)?;
                let count = components
                    .checked_mul(tuples)
                    .ok_or_else(|| FormatError::new("FIELD array size overflow"))?;
                let values = reader.floats(count, kind)?;
                if tuples == tuple_values(grid, current, 1)? {
                    grid.offer_field(&name, current, components, Ok(values));
                } else {
                    grid.skipped_fields.push(format!(
                        "{name} ({}): {tuples} tuples, expected {}",
                        current.as_str(),
                        tuple_values(grid, current, 1)?
                    ));
                }
                if reader
                    .cursor
                    .peek_token()
                    .is_some_and(|token| token.eq_ignore_ascii_case("METADATA"))
                {
                    reader.cursor.token()?;
                    skip_metadata(&mut reader.cursor)?;
                }
            }
        }
        other => {
            return Err(FormatError::new(format!("unsupported attribute {other}")));
        }
    }
    Ok(())
}

fn tuple_values(
    grid: &RawGrid,
    association: panta_mesh::FieldAssociation,
    components: usize,
) -> FormatResult<usize> {
    let tuples = match association {
        panta_mesh::FieldAssociation::Node => grid.points.len(),
        panta_mesh::FieldAssociation::Cell => grid.cell_count(),
    };
    tuples
        .checked_mul(components)
        .ok_or_else(|| FormatError::new("attribute size overflow"))
}

/// VTK 5.1 把名称中的空格等字符写成 `%XX`；解码失败的序列原样保留。
fn decode_name(token: &str) -> String {
    let bytes = token.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(value) = token
                .get(index + 1..index + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            decoded.push(value);
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(decoded).unwrap_or_else(|_| token.to_owned())
}

/// METADATA 块以空行结束；仅含名称和信息键，不影响几何。
fn skip_metadata(cursor: &mut Cursor<'_>) -> FormatResult<()> {
    cursor.finish_line()?;
    loop {
        if cursor.line()?.trim().is_empty() {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_vtk_legacy;

    #[test]
    fn legacy_42_polydata_polygons_are_read() -> Result<(), super::FormatError> {
        let mesh = parse_vtk_legacy(
            b"# vtk DataFile Version 4.2\nsquare\nASCII\nDATASET POLYDATA\nPOINTS 4 float\n0 0 0 1 0 0 1 1 0 0 1 0\nPOLYGONS 1 5\n4 0 1 2 3\nPOINT_DATA 4\n",
        )?;
        assert_eq!(mesh.triangles.len(), 2);
        Ok(())
    }

    const TET_GRID: &str = "# vtk DataFile Version 4.2\ntet\nASCII\nDATASET UNSTRUCTURED_GRID\nPOINTS 4 double\n0 0 0 1 0 0 0 1 0 0 0 1\nCELLS 1 5\n4 0 1 2 3\nCELL_TYPES 1\n10\n";

    #[test]
    fn legacy_attributes_become_result_fields() -> Result<(), super::FormatError> {
        use panta_mesh::FieldAssociation;
        let text = format!(
            "{TET_GRID}CELL_DATA 1\nSCALARS material int 1\nLOOKUP_TABLE default\n2\n\
             TENSORS stress double\n1 0 0 0 1 0 0 0 1\n\
             POINT_DATA 4\nSCALARS wall%20temp float\nLOOKUP_TABLE default\n1 2 inf 4\n\
             VECTORS U double\n1 0 0 0 1 0 0 0 1 0 0 0\n\
             FIELD FieldData 2\nk 1 4 double\n1 1 1 1\nMETADATA\nINFORMATION 0\n\n\
             pair 2 4 float\n0 0 0 0 0 0 0 0\n"
        );
        let grid = super::parse_vtk_legacy_grid(text.as_bytes())?;
        let names: Vec<(&str, FieldAssociation, u8)> = grid
            .fields
            .iter()
            .map(|field| (field.name.as_str(), field.association, field.components))
            .collect();
        assert_eq!(
            names,
            [
                ("material", FieldAssociation::Cell, 1),
                ("wall temp", FieldAssociation::Node, 1),
                ("U", FieldAssociation::Node, 3),
                ("k", FieldAssociation::Node, 1),
            ]
        );
        assert_eq!(grid.fields[1].summary().missing, 1);
        assert_eq!(grid.skipped_fields.len(), 2, "{:?}", grid.skipped_fields);
        assert!(grid.skipped_fields[0].starts_with("stress (cell): TENSORS"));
        assert!(grid.skipped_fields[1].starts_with("pair (node): 2 components"));
        Ok(())
    }

    #[test]
    fn legacy_binary_scalars_are_read_and_bad_sections_only_drop_fields()
    -> Result<(), super::FormatError> {
        let mut bytes = b"# vtk DataFile Version 4.2\ntet\nBINARY\nDATASET UNSTRUCTURED_GRID\nPOINTS 4 double\n".to_vec();
        for value in [0.0f64, 0., 0., 1., 0., 0., 0., 1., 0., 0., 0., 1.] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.extend_from_slice(b"\nCELLS 1 5\n");
        for value in [4i32, 0, 1, 2, 3] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.extend_from_slice(b"\nCELL_TYPES 1\n");
        bytes.extend_from_slice(&10i32.to_be_bytes());
        bytes.extend_from_slice(b"\nPOINT_DATA 4\nSCALARS t float 1\nLOOKUP_TABLE default\n");
        for value in [1.5f32, 2.5, 3.5, 4.5] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.extend_from_slice(b"\n");
        let grid = super::parse_vtk_legacy_grid(&bytes)?;
        assert_eq!(grid.fields.len(), 1, "{:?}", grid.skipped_fields);
        assert_eq!(grid.fields[0].values, [1.5, 2.5, 3.5, 4.5]);
        assert!(grid.skipped_fields.is_empty());

        let mut bytes = TET_GRID.as_bytes().to_vec();
        bytes.extend_from_slice(b"POINT_DATA 4\nSCALARS t double\nLOOKUP_TABLE default\n1 2 3\n");
        let grid = super::parse_vtk_legacy_grid(&bytes)?;
        assert!(grid.fields.is_empty());
        assert_eq!(grid.skipped_fields.len(), 1, "{:?}", grid.skipped_fields);
        assert!(grid.skipped_fields[0].contains("remaining attributes ignored"));
        let mut bytes = TET_GRID.as_bytes().to_vec();
        bytes.extend_from_slice(b"POINT_DATA 3\n");
        let grid = super::parse_vtk_legacy_grid(&bytes)?;
        assert_eq!(grid.cell_count(), 1);
        assert_eq!(grid.skipped_fields.len(), 1);
        Ok(())
    }

    #[test]
    fn unknown_dataset_and_bad_indices_are_rejected() {
        assert!(
            parse_vtk_legacy(b"# vtk DataFile Version 4.2\nx\nASCII\nDATASET STRUCTURED_POINTS\n")
                .is_err()
        );
        assert!(parse_vtk_legacy(
            b"# vtk DataFile Version 4.2\nx\nASCII\nDATASET POLYDATA\nPOINTS 3 float\n0 0 0 1 0 0 0 1 0\nPOLYGONS 1 4\n3 0 1 9\n"
        )
        .is_err());
    }
}
