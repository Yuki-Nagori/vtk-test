//! VTK XML `UnstructuredGrid`（.vtu）与 `PolyData`（.vtp）。支持 ascii、inline binary
//! （base64）和 appended（raw / base64）数据，UInt32 / UInt64 头，zlib 压缩。
//!
//! 只解析几何所需的 Points、Cells / Polys / Strips；`AppendedData` 可能包含任意字节，
//! 因此不交给通用 XML 解析器，而是先定位其起点，只扫描之前的标签。

use std::collections::HashMap;
use std::io::Read;

use panta_mesh::SurfaceMesh;

use super::cells::CellSurface;
use super::text::{Scalar, decode_f64_array, decode_index_array, points_from_flat};
use super::{CellList, FormatError, FormatResult, RawGrid, TriangleSink, finish_mesh};

pub fn parse_vtu(bytes: &[u8]) -> FormatResult<SurfaceMesh> {
    parse_xml(bytes, "UnstructuredGrid")
}

/// VTU 线性四面体体网格的原始节点与单元（task 021）。
pub fn parse_vtu_grid(bytes: &[u8]) -> FormatResult<RawGrid> {
    let mut grid = RawGrid::default();
    read_vtk_xml(
        bytes,
        "UnstructuredGrid",
        &mut SurfaceMesh {
            triangles: Vec::new(),
        },
        Some(&mut grid),
    )?;
    Ok(grid)
}

pub fn parse_vtp(bytes: &[u8]) -> FormatResult<SurfaceMesh> {
    parse_xml(bytes, "PolyData")
}

fn parse_xml(bytes: &[u8], dataset: &str) -> FormatResult<SurfaceMesh> {
    let mut mesh = SurfaceMesh {
        triangles: Vec::new(),
    };
    read_vtk_xml(bytes, dataset, &mut mesh, None)?;
    finish_mesh(mesh)
}

#[derive(Debug)]
struct Tag {
    name: String,
    attributes: HashMap<String, String>,
    closing: bool,
    self_closing: bool,
    /// 起始标签之后、下一个 `<` 之前的文本（inline 数据）。
    text_start: usize,
    text_end: usize,
}

/// 扫描 `[0, limit)` 内的标签；跳过注释和处理指令。
fn scan_tags(bytes: &[u8], limit: usize) -> FormatResult<Vec<Tag>> {
    let mut tags = Vec::new();
    let mut position = 0;
    while let Some(offset) = bytes[position..limit].iter().position(|&byte| byte == b'<') {
        let start = position + offset;
        if bytes[start..limit].starts_with(b"<!--") {
            let end = find(&bytes[start..limit], b"-->")
                .ok_or_else(|| FormatError::new("unterminated XML comment"))?;
            position = start + end + 3;
            continue;
        }
        let end = start
            + bytes[start..limit]
                .iter()
                .position(|&byte| byte == b'>')
                .ok_or_else(|| FormatError::new("unterminated XML tag"))?;
        let body = std::str::from_utf8(&bytes[start + 1..end])
            .map_err(|_| FormatError::new("XML tag is not UTF-8"))?;
        position = end + 1;
        if body.starts_with('?') || body.starts_with('!') {
            continue;
        }
        let closing = body.starts_with('/');
        let self_closing = body.ends_with('/');
        let body = body.trim_start_matches('/').trim_end_matches('/');
        let mut parts = body.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or_default().to_owned();
        let attributes = parse_attributes(parts.next().unwrap_or_default())?;
        let text_end = bytes[position..limit]
            .iter()
            .position(|&byte| byte == b'<')
            .map_or(limit, |offset| position + offset);
        tags.push(Tag {
            name,
            attributes,
            closing,
            self_closing,
            text_start: position,
            text_end,
        });
    }
    Ok(tags)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn parse_attributes(text: &str) -> FormatResult<HashMap<String, String>> {
    let mut attributes = HashMap::new();
    let mut rest = text.trim();
    while !rest.is_empty() {
        let (key, after) = rest
            .split_once('=')
            .ok_or_else(|| FormatError::new("malformed XML attribute"))?;
        let after = after.trim_start();
        let quote = after
            .chars()
            .next()
            .filter(|quote| *quote == '"' || *quote == '\'')
            .ok_or_else(|| FormatError::new("unquoted XML attribute"))?;
        let value_end = after[1..]
            .find(quote)
            .ok_or_else(|| FormatError::new("unterminated XML attribute"))?;
        attributes.insert(key.trim().to_owned(), after[1..1 + value_end].to_owned());
        rest = after[value_end + 2..].trim_start();
    }
    Ok(attributes)
}

struct Context<'a> {
    bytes: &'a [u8],
    little_endian: bool,
    header: Scalar,
    compressed: bool,
    /// AppendedData 中 `_` 之后第一个字节的位置，以及是否为 base64 编码。
    appended: Option<(usize, bool)>,
}

pub(crate) fn read_vtk_xml(
    bytes: &[u8],
    dataset: &str,
    sink: &mut impl TriangleSink,
    grid: Option<&mut RawGrid>,
) -> FormatResult<()> {
    let appended_tag = find(bytes, b"<AppendedData");
    let limit = appended_tag.unwrap_or(bytes.len());
    let tags = scan_tags(bytes, limit)?;
    let root = tags
        .iter()
        .find(|tag| tag.name == "VTKFile" && !tag.closing)
        .ok_or_else(|| FormatError::new("missing VTKFile element"))?;
    if root.attributes.get("type").map(String::as_str) != Some(dataset) {
        return Err(FormatError::new(format!("VTKFile type is not {dataset}")));
    }
    let little_endian = root.attributes.get("byte_order").map(String::as_str) != Some("BigEndian");
    let header = match root.attributes.get("header_type").map(String::as_str) {
        None | Some("UInt32") => Scalar::U32,
        Some("UInt64") => Scalar::U64,
        Some(other) => return Err(FormatError::new(format!("unsupported header_type {other}"))),
    };
    let compressed = match root.attributes.get("compressor").map(String::as_str) {
        None | Some("") => false,
        Some("vtkZLibDataCompressor") => true,
        Some(other) => return Err(FormatError::new(format!("unsupported compressor {other}"))),
    };
    let appended = match appended_tag {
        Some(start) => {
            let tag_end = start
                + bytes[start..]
                    .iter()
                    .position(|&byte| byte == b'>')
                    .ok_or_else(|| FormatError::new("unterminated AppendedData"))?;
            let tag = std::str::from_utf8(&bytes[start + 1..tag_end])
                .map_err(|_| FormatError::new("XML tag is not UTF-8"))?;
            let encoding = parse_attributes(tag.trim_start_matches("AppendedData"))?
                .get("encoding")
                .cloned()
                .unwrap_or_else(|| "raw".to_owned());
            let underscore = tag_end
                + 1
                + bytes[tag_end + 1..]
                    .iter()
                    .position(|&byte| byte == b'_')
                    .ok_or_else(|| FormatError::new("AppendedData missing '_' marker"))?;
            Some((underscore + 1, encoding == "base64"))
        }
        None => None,
    };
    let context = Context {
        bytes,
        little_endian,
        header,
        compressed,
        appended,
    };

    let mut section = String::new();
    let mut arrays: Vec<(String, &Tag)> = Vec::new();
    let mut pieces = 0usize;
    for tag in &tags {
        match (tag.name.as_str(), tag.closing) {
            ("Piece", false) => {
                pieces += 1;
                if pieces > 1 {
                    return Err(FormatError::new(
                        "multiple Piece elements are not supported",
                    ));
                }
            }
            (
                "Points" | "Cells" | "Polys" | "Strips" | "Verts" | "Lines" | "PointData"
                | "CellData" | "FieldData",
                false,
            ) if !tag.self_closing => section.clone_from(&tag.name),
            (
                "Points" | "Cells" | "Polys" | "Strips" | "Verts" | "Lines" | "PointData"
                | "CellData" | "FieldData",
                true,
            ) => section.clear(),
            ("DataArray", false) => arrays.push((section.clone(), tag)),
            _ => {}
        }
    }
    let array = |section: &str, name: Option<&str>| {
        arrays
            .iter()
            .filter(|(owner, _)| owner == section)
            .map(|(_, tag)| *tag)
            .find(|tag| {
                name.is_none_or(|name| tag.attributes.get("Name").map(String::as_str) == Some(name))
            })
    };
    let points_tag =
        array("Points", None).ok_or_else(|| FormatError::new("missing Points DataArray"))?;
    let points = points_from_flat(&context.floats(points_tag)?)?;
    if let Some(grid) = grid {
        let connectivity = array("Cells", Some("connectivity"))
            .ok_or_else(|| FormatError::new("missing Cells connectivity"))?;
        let cells = context.cells(
            connectivity,
            array("Cells", Some("offsets"))
                .ok_or_else(|| FormatError::new("missing Cells offsets"))?,
        )?;
        let types = context.indices(
            array("Cells", Some("types")).ok_or_else(|| FormatError::new("missing Cells types"))?,
        )?;
        *grid = RawGrid::new(points, cells, types)?;
        for (section, tag) in &arrays {
            let association = match section.as_str() {
                "PointData" => panta_mesh::FieldAssociation::Node,
                "CellData" => panta_mesh::FieldAssociation::Cell,
                _ => continue,
            };
            let name = tag.attributes.get("Name").map_or("", String::as_str);
            let components = tag
                .attributes
                .get("NumberOfComponents")
                .map_or(Some(1), |value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            let values = if matches!(components, 1 | 3) {
                context.floats(tag)
            } else {
                Ok(Vec::new())
            };
            grid.offer_field(name, association, components, values);
        }
        return Ok(());
    }
    let mut surface = CellSurface::new(&points);
    if dataset == "UnstructuredGrid" {
        if let Some(connectivity) = array("Cells", Some("connectivity")) {
            let cells = context.cells(
                connectivity,
                array("Cells", Some("offsets"))
                    .ok_or_else(|| FormatError::new("missing Cells offsets"))?,
            )?;
            let types = context.indices(
                array("Cells", Some("types"))
                    .ok_or_else(|| FormatError::new("missing Cells types"))?,
            )?;
            if types.len() != cells.len() {
                return Err(FormatError::new("Cells types count mismatch"));
            }
            for (kind, nodes) in types.iter().zip(cells.iter()) {
                let kind =
                    u8::try_from(*kind).map_err(|_| FormatError::new("invalid cell type"))?;
                surface.add(kind, nodes, sink)?;
            }
        }
    } else {
        for (section, strip) in [("Polys", false), ("Strips", true)] {
            let Some(connectivity) = array(section, Some("connectivity")) else {
                continue;
            };
            let offsets = array(section, Some("offsets"))
                .ok_or_else(|| FormatError::new(format!("missing {section} offsets")))?;
            for nodes in context.cells(connectivity, offsets)?.iter() {
                if strip {
                    surface.points().strip(nodes, sink)?;
                } else {
                    surface.points().polygon(nodes, sink)?;
                }
            }
        }
    }
    surface.finish(sink)
}

fn scalar(name: &str) -> FormatResult<Scalar> {
    Ok(match name {
        "Int8" => Scalar::I8,
        "UInt8" => Scalar::U8,
        "Int16" => Scalar::I16,
        "UInt16" => Scalar::U16,
        "Int32" => Scalar::I32,
        "UInt32" => Scalar::U32,
        "Int64" => Scalar::I64,
        "UInt64" => Scalar::U64,
        "Float32" => Scalar::F32,
        "Float64" => Scalar::F64,
        other => {
            return Err(FormatError::new(format!(
                "unsupported DataArray type {other}"
            )));
        }
    })
}

impl Context<'_> {
    fn floats(&self, tag: &Tag) -> FormatResult<Vec<f64>> {
        let kind = array_type(tag)?;
        match self.payload(tag)? {
            Payload::Ascii(text) => text
                .split_ascii_whitespace()
                .map(|value| {
                    value
                        .parse::<f64>()
                        .map_err(|_| FormatError::new("invalid ascii number"))
                })
                .collect(),
            Payload::Binary(bytes) => {
                decode_f64_array(&bytes, kind, bytes.len() / kind.size(), self.little_endian)
            }
        }
    }

    fn indices(&self, tag: &Tag) -> FormatResult<Vec<u64>> {
        let kind = array_type(tag)?;
        match self.payload(tag)? {
            Payload::Ascii(text) => text
                .split_ascii_whitespace()
                .map(|value| {
                    value
                        .parse::<u64>()
                        .map_err(|_| FormatError::new("invalid ascii index"))
                })
                .collect(),
            Payload::Binary(bytes) => {
                decode_index_array(&bytes, kind, bytes.len() / kind.size(), self.little_endian)
            }
        }
    }

    fn cells(&self, connectivity: &Tag, offsets: &Tag) -> FormatResult<CellList> {
        CellList::from_ends(self.indices(connectivity)?, &self.indices(offsets)?)
    }

    fn payload<'t>(&self, tag: &'t Tag) -> FormatResult<Payload<'t>>
    where
        Self: 't,
    {
        let format = tag.attributes.get("format").map_or("ascii", String::as_str);
        match format {
            "ascii" => std::str::from_utf8(&self.bytes[tag.text_start..tag.text_end])
                .map(Payload::Ascii)
                .map_err(|_| FormatError::new("ascii DataArray is not UTF-8")),
            "binary" => {
                let text = trim_ascii(&self.bytes[tag.text_start..tag.text_end]);
                self.decode_base64_block(text).map(Payload::Binary)
            }
            "appended" => {
                let (start, base64) = self
                    .appended
                    .ok_or_else(|| FormatError::new("missing AppendedData"))?;
                let offset: usize = tag
                    .attributes
                    .get("offset")
                    .and_then(|offset| offset.parse().ok())
                    .ok_or_else(|| FormatError::new("appended DataArray without offset"))?;
                let begin = start
                    .checked_add(offset)
                    .filter(|&begin| begin <= self.bytes.len())
                    .ok_or_else(|| FormatError::new("appended offset out of range"))?;
                let rest = &self.bytes[begin..];
                if base64 {
                    let end = rest
                        .iter()
                        .position(|byte| !is_base64(*byte))
                        .unwrap_or(rest.len());
                    self.decode_base64_block(&rest[..end]).map(Payload::Binary)
                } else {
                    self.decode_raw_block(rest).map(Payload::Binary)
                }
            }
            other => Err(FormatError::new(format!(
                "unsupported DataArray format {other}"
            ))),
        }
    }

    fn header_values(&self, bytes: &[u8], count: usize) -> FormatResult<Vec<usize>> {
        decode_index_array(bytes, self.header, count, self.little_endian)?
            .into_iter()
            .map(|value| usize::try_from(value).map_err(|_| FormatError::new("block too large")))
            .collect()
    }

    /// raw appended：紧随偏移的是头和（可能压缩的）数据。
    fn decode_raw_block(&self, bytes: &[u8]) -> FormatResult<Vec<u8>> {
        let size = self.header.size();
        if !self.compressed {
            let length = self.header_values(bytes, 1)?[0];
            return bytes
                .get(size..size + length)
                .map(<[u8]>::to_vec)
                .ok_or_else(|| FormatError::new("appended block is truncated"));
        }
        let blocks = self.header_values(bytes, 1)?[0];
        let header_length = (3 + blocks) * size;
        let header = self.header_values(
            bytes
                .get(..header_length)
                .ok_or_else(|| FormatError::new("compression header is truncated"))?,
            3 + blocks,
        )?;
        let payload_length: usize = header[3..].iter().sum();
        let payload = bytes
            .get(header_length..header_length + payload_length)
            .ok_or_else(|| FormatError::new("compressed block is truncated"))?;
        inflate_blocks(&header, payload)
    }

    /// base64：未压缩时头与数据一起编码；压缩时头单独编码，随后是压缩数据。
    fn decode_base64_block(&self, text: &[u8]) -> FormatResult<Vec<u8>> {
        let size = self.header.size();
        if !self.compressed {
            let decoded = decode_base64(text)?;
            let length = self.header_values(&decoded, 1)?[0];
            return decoded
                .get(size..size + length)
                .map(<[u8]>::to_vec)
                .ok_or_else(|| FormatError::new("binary block is truncated"));
        }
        let first = decode_base64(
            text.get(..base64_length(size))
                .ok_or_else(|| FormatError::new("compression header is truncated"))?,
        )?;
        let blocks = self.header_values(&first, 1)?[0];
        let header_chars = base64_length((3 + blocks) * size);
        let header_bytes = decode_base64(
            text.get(..header_chars)
                .ok_or_else(|| FormatError::new("compression header is truncated"))?,
        )?;
        let header = self.header_values(&header_bytes, 3 + blocks)?;
        let payload = decode_base64(&text[header_chars..])?;
        inflate_blocks(&header, &payload)
    }
}

enum Payload<'a> {
    Ascii(&'a str),
    Binary(Vec<u8>),
}

fn array_type(tag: &Tag) -> FormatResult<Scalar> {
    scalar(
        tag.attributes
            .get("type")
            .map(String::as_str)
            .ok_or_else(|| FormatError::new("DataArray without type"))?,
    )
}

/// 压缩头为 `[块数, 块大小, 末块大小, 各块压缩大小...]`。
fn inflate_blocks(header: &[usize], payload: &[u8]) -> FormatResult<Vec<u8>> {
    let (blocks, block_size, last_size) = (header[0], header[1], header[2]);
    let mut output = Vec::new();
    let mut position = 0usize;
    for (index, &compressed) in header[3..3 + blocks].iter().enumerate() {
        let expected = if index + 1 == blocks && last_size != 0 {
            last_size
        } else {
            block_size
        };
        let chunk = payload
            .get(position..position + compressed)
            .ok_or_else(|| FormatError::new("compressed block is truncated"))?;
        let before = output.len();
        flate2::read::ZlibDecoder::new(chunk)
            .take(expected as u64)
            .read_to_end(&mut output)
            .map_err(|error| FormatError::new(format!("zlib: {error}")))?;
        if output.len() - before != expected {
            return Err(FormatError::new("decompressed block size mismatch"));
        }
        position += compressed;
    }
    Ok(output)
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(start, |end| end + 1);
    &bytes[start..end]
}

fn base64_length(bytes: usize) -> usize {
    bytes.div_ceil(3) * 4
}

fn is_base64(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=')
}

fn decode_base64(text: &[u8]) -> FormatResult<Vec<u8>> {
    let value = |byte: u8| -> FormatResult<u32> {
        Ok(match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a') + 26,
            b'0'..=b'9' => u32::from(byte - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err(FormatError::new("invalid base64")),
        })
    };
    let clean: Vec<u8> = text
        .iter()
        .copied()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    if !clean.len().is_multiple_of(4) {
        return Err(FormatError::new("base64 length is not a multiple of 4"));
    }
    let mut output = Vec::with_capacity(clean.len() / 4 * 3);
    for quad in clean.as_chunks::<4>().0 {
        let padding = quad.iter().rev().take_while(|&&byte| byte == b'=').count();
        let mut bits = 0u32;
        for &byte in &quad[..4 - padding] {
            bits = (bits << 6) | value(byte)?;
        }
        bits <<= 6 * padding as u32;
        let decoded = [(bits >> 16) as u8, (bits >> 8) as u8, bits as u8];
        output.extend_from_slice(&decoded[..3 - padding]);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{decode_base64, parse_vtp, parse_vtu};

    #[test]
    fn base64_round_trips_padding() -> Result<(), super::FormatError> {
        assert_eq!(decode_base64(b"TWFu")?, b"Man");
        assert_eq!(decode_base64(b"TWE=")?, b"Ma");
        assert_eq!(decode_base64(b"TQ==")?, b"M");
        assert!(decode_base64(b"TQ=").is_err());
        Ok(())
    }

    #[test]
    fn ascii_vtu_tetrahedron_exposes_four_faces() -> Result<(), super::FormatError> {
        let mesh = parse_vtu(
            br#"<?xml version="1.0"?>
<VTKFile type="UnstructuredGrid" version="1.0" byte_order="LittleEndian">
 <UnstructuredGrid><Piece NumberOfPoints="4" NumberOfCells="1">
  <Points><DataArray type="Float32" NumberOfComponents="3" format="ascii">0 0 0 1 0 0 0 1 0 0 0 1</DataArray></Points>
  <Cells>
   <DataArray type="Int64" Name="connectivity" format="ascii">0 1 2 3</DataArray>
   <DataArray type="Int64" Name="offsets" format="ascii">4</DataArray>
   <DataArray type="UInt8" Name="types" format="ascii">10</DataArray>
  </Cells>
 </Piece></UnstructuredGrid>
</VTKFile>"#,
        )?;
        assert_eq!(mesh.triangles.len(), 4);
        Ok(())
    }

    #[test]
    fn vtu_point_and_cell_arrays_become_result_fields() -> Result<(), super::FormatError> {
        use panta_mesh::FieldAssociation;
        let grid = super::parse_vtu_grid(
            br#"<?xml version="1.0"?>
<VTKFile type="UnstructuredGrid" version="1.0" byte_order="LittleEndian">
 <UnstructuredGrid><Piece NumberOfPoints="4" NumberOfCells="1">
  <PointData Scalars="p">
   <DataArray type="Float64" Name="p" format="ascii">1 nan 3 4</DataArray>
   <DataArray type="Float32" Name="uv" NumberOfComponents="2" format="ascii">0 0 0 0 0 0 0 0</DataArray>
   <DataArray type="Float32" Name="short" format="ascii">1 2</DataArray>
  </PointData>
  <CellData>
   <DataArray type="Float32" Name="U" NumberOfComponents="3" format="ascii">3 4 0</DataArray>
   <DataArray type="Int32" Name="material" format="ascii">7</DataArray>
   <DataArray type="String" Name="label" format="ascii">x</DataArray>
  </CellData>
  <Points><DataArray type="Float32" NumberOfComponents="3" format="ascii">0 0 0 1 0 0 0 1 0 0 0 1</DataArray></Points>
  <Cells>
   <DataArray type="Int64" Name="connectivity" format="ascii">0 1 2 3</DataArray>
   <DataArray type="Int64" Name="offsets" format="ascii">4</DataArray>
   <DataArray type="UInt8" Name="types" format="ascii">10</DataArray>
  </Cells>
 </Piece></UnstructuredGrid>
</VTKFile>"#,
        )?;
        let names: Vec<(&str, FieldAssociation, u8)> = grid
            .fields
            .iter()
            .map(|field| (field.name.as_str(), field.association, field.components))
            .collect();
        assert_eq!(
            names,
            [
                ("p", FieldAssociation::Node, 1),
                ("U", FieldAssociation::Cell, 3),
                ("material", FieldAssociation::Cell, 1),
            ]
        );
        assert!(grid.fields[0].values[1].is_nan());
        assert_eq!(grid.fields[0].summary().missing, 1);
        assert_eq!(grid.fields[1].summary().range, Some([5.0, 5.0]));
        assert_eq!(grid.skipped_fields.len(), 3, "{:?}", grid.skipped_fields);
        assert!(grid.skipped_fields[0].starts_with("uv (node): 2 components"));
        assert!(grid.skipped_fields[1].starts_with("short (node): 2 values"));
        assert!(grid.skipped_fields[2].starts_with("label (cell)"));
        Ok(())
    }

    #[test]
    fn mismatched_dataset_type_is_rejected() {
        assert!(parse_vtp(br#"<VTKFile type="UnstructuredGrid"></VTKFile>"#).is_err());
    }
}
