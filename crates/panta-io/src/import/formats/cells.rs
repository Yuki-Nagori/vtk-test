//! VTK 单元类型到表面三角形的转换；局部节点与面顺序沿用 VTK 单元定义。

use std::collections::HashMap;

use super::{FormatError, FormatResult, IndexedPoints, TriangleSink};

const VERTEX: u8 = 1;
const POLY_VERTEX: u8 = 2;
const LINE: u8 = 3;
const POLY_LINE: u8 = 4;
const TRIANGLE: u8 = 5;
const TRIANGLE_STRIP: u8 = 6;
const POLYGON: u8 = 7;
const PIXEL: u8 = 8;
const QUAD: u8 = 9;
const TETRA: u8 = 10;
const VOXEL: u8 = 11;
const HEXAHEDRON: u8 = 12;
const WEDGE: u8 = 13;
const PYRAMID: u8 = 14;
const QUADRATIC_EDGE: u8 = 21;
const QUADRATIC_TRIANGLE: u8 = 22;
const QUADRATIC_QUAD: u8 = 23;
const QUADRATIC_TETRA: u8 = 24;
const QUADRATIC_HEXAHEDRON: u8 = 25;

const TETRA_FACES: &[&[usize]] = &[&[0, 1, 3], &[1, 2, 3], &[2, 0, 3], &[0, 2, 1]];
const HEXAHEDRON_FACES: &[&[usize]] = &[
    &[0, 4, 7, 3],
    &[1, 2, 6, 5],
    &[0, 1, 5, 4],
    &[3, 7, 6, 2],
    &[0, 3, 2, 1],
    &[4, 5, 6, 7],
];
const WEDGE_FACES: &[&[usize]] = &[
    &[0, 1, 2],
    &[3, 5, 4],
    &[0, 3, 4, 1],
    &[1, 4, 5, 2],
    &[2, 5, 3, 0],
];
const PYRAMID_FACES: &[&[usize]] = &[
    &[0, 3, 2, 1],
    &[0, 1, 4],
    &[1, 2, 4],
    &[2, 3, 4],
    &[3, 0, 4],
];
/// 体素按 VTK 定义是轴对齐的六面体，其节点 2/3、6/7 与六面体编号互换。
const VOXEL_TO_HEXAHEDRON: [usize; 8] = [0, 1, 3, 2, 4, 5, 7, 6];

/// 体单元面最多 4 个节点；不足 4 个的位置用 `u64::MAX` 填充。
type FaceNodes = [u64; 4];

/// 二维单元直接输出；体单元面按排序后的节点集合做奇偶配对：第二次出现即为内部面并移除，
/// `finish` 只输出剩余（出现奇数次）的外表面。配对后移除让常驻内存只与未配对面
/// （外表面和遍历前沿）成正比，而不是与全部体单元面成正比。
pub(crate) struct CellSurface<'a> {
    points: IndexedPoints<'a>,
    next_sequence: u64,
    open_faces: HashMap<FaceNodes, (u64, FaceNodes)>,
}

impl<'a> CellSurface<'a> {
    pub fn new(points: &'a [[f64; 3]]) -> Self {
        Self {
            points: IndexedPoints { points },
            next_sequence: 0,
            open_faces: HashMap::new(),
        }
    }

    pub fn points(&self) -> &IndexedPoints<'a> {
        &self.points
    }

    pub fn add(
        &mut self,
        cell_type: u8,
        nodes: &[u64],
        sink: &mut impl TriangleSink,
    ) -> FormatResult<()> {
        let corners = |count: usize| -> FormatResult<&[u64]> {
            nodes
                .get(..count)
                .ok_or_else(|| FormatError::new(format!("cell type {cell_type} has too few nodes")))
        };
        match cell_type {
            VERTEX | POLY_VERTEX | LINE | POLY_LINE | QUADRATIC_EDGE => Ok(()),
            TRIANGLE => self.points.polygon(corners(3)?, sink),
            QUADRATIC_TRIANGLE => self.points.polygon(corners(3)?, sink),
            QUAD => self.points.polygon(corners(4)?, sink),
            QUADRATIC_QUAD => self.points.polygon(corners(4)?, sink),
            POLYGON => self.points.polygon(nodes, sink),
            TRIANGLE_STRIP => self.points.strip(nodes, sink),
            PIXEL => {
                let n = corners(4)?;
                self.points.polygon(&[n[0], n[1], n[3], n[2]], sink)
            }
            TETRA | QUADRATIC_TETRA => self.add_volume(corners(4)?, TETRA_FACES),
            HEXAHEDRON | QUADRATIC_HEXAHEDRON => self.add_volume(corners(8)?, HEXAHEDRON_FACES),
            VOXEL => {
                let n = corners(8)?;
                let reordered = VOXEL_TO_HEXAHEDRON.map(|index| n[index]);
                self.add_volume(&reordered, HEXAHEDRON_FACES)
            }
            WEDGE => self.add_volume(corners(6)?, WEDGE_FACES),
            PYRAMID => self.add_volume(corners(5)?, PYRAMID_FACES),
            other => Err(FormatError::new(format!(
                "unsupported VTK cell type {other}"
            ))),
        }
    }

    fn add_volume(&mut self, nodes: &[u64], faces: &[&[usize]]) -> FormatResult<()> {
        for face in faces {
            let mut ordered = [u64::MAX; 4];
            for (slot, &local) in ordered.iter_mut().zip(face.iter()) {
                *slot = nodes[local];
            }
            let mut key = ordered;
            key.sort_unstable();
            if self.open_faces.remove(&key).is_none() {
                self.open_faces.insert(key, (self.next_sequence, ordered));
                self.next_sequence += 1;
            }
        }
        Ok(())
    }

    /// 按首次出现顺序输出外表面，保证结果与哈希顺序无关。
    pub fn finish(self, sink: &mut impl TriangleSink) -> FormatResult<()> {
        let mut boundary: Vec<(u64, FaceNodes)> = self.open_faces.into_values().collect();
        boundary.sort_unstable_by_key(|(sequence, _)| *sequence);
        for (_, face) in boundary {
            let length = face.iter().take_while(|&&node| node != u64::MAX).count();
            self.points.polygon(&face[..length], sink)?;
        }
        Ok(())
    }
}
