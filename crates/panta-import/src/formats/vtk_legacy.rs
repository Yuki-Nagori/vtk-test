//! 旧版 VTK（`.vtk`）ASCII / BINARY 的 POLYDATA 与 UNSTRUCTURED_GRID；同时支持 5.1 版
//! OFFSETS / CONNECTIVITY 单元布局和 5.1 之前的计数前缀布局。点、单元以外的属性数据忽略。

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
            "POINT_DATA" | "CELL_DATA" | "FIELD" => break,
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
        *grid = RawGrid::new(points, cells, types)?;
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
