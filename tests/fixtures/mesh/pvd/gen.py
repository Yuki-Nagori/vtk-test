#!/usr/bin/env python3
"""生成 `.pvd` 时间序列夹具（任务 033）：一个六面体 + 顶部四面体，三步 ascii VTU。"""
from pathlib import Path

POINTS = [
    (0, 0, 0), (1, 0, 0), (1, 1, 0), (0, 1, 0),
    (0, 0, 1), (1, 0, 1), (1, 1, 1), (0, 1, 1),
    (0.5, 0.5, 2),
]
CELLS = [([0, 1, 2, 3, 4, 5, 6, 7], 12), ([4, 5, 6, 8], 10)]


def vtu(time, points=POINTS, extra=False):
    point_values = " ".join(f"{100 * time + index:g}" for index in range(len(points)))
    cell_values = " ".join(f"{time + index:g}" for index in range(len(CELLS)))
    connectivity = " ".join(str(node) for nodes, _ in CELLS for node in nodes)
    offsets, total = [], 0
    for nodes, _ in CELLS:
        total += len(nodes)
        offsets.append(str(total))
    types = " ".join(str(kind) for _, kind in CELLS)
    coords = " ".join(f"{x:g} {y:g} {z:g}" for x, y, z in points)
    init = (
        '        <DataArray type="Float64" Name="init" format="ascii">7 8</DataArray>\n'
        if extra
        else ""
    )
    return f"""<?xml version="1.0"?>
<VTKFile type="UnstructuredGrid" version="1.0" byte_order="LittleEndian">
  <UnstructuredGrid>
    <Piece NumberOfPoints="{len(points)}" NumberOfCells="{len(CELLS)}">
      <PointData>
        <DataArray type="Float64" Name="T" format="ascii">{point_values}</DataArray>
      </PointData>
      <CellData>
        <DataArray type="Float64" Name="p" format="ascii">{cell_values}</DataArray>
{init}      </CellData>
      <Points>
        <DataArray type="Float64" NumberOfComponents="3" format="ascii">{coords}</DataArray>
      </Points>
      <Cells>
        <DataArray type="Int64" Name="connectivity" format="ascii">{connectivity}</DataArray>
        <DataArray type="Int64" Name="offsets" format="ascii">{' '.join(offsets)}</DataArray>
        <DataArray type="UInt8" Name="types" format="ascii">{types}</DataArray>
      </Cells>
    </Piece>
  </UnstructuredGrid>
</VTKFile>
"""


def collection(entries):
    rows = "".join(
        f'    <DataSet timestep="{time}" group="" part="0" file="{file}"/>\n'
        for time, file in entries
    )
    return f"""<?xml version="1.0"?>
<VTKFile type="Collection" version="0.1" byte_order="LittleEndian">
  <Collection>
{rows}  </Collection>
</VTKFile>
"""


root = Path(__file__).parent
(root / "steps").mkdir(exist_ok=True)
for index, time in enumerate([0, 0.5, 1]):
    (root / f"steps/flow_{index}.vtu").write_text(vtu(time, extra=index == 0))
# 故意乱序列出：导入按 timestep 升序排列。
(root / "flow.pvd").write_text(
    collection([("1", "steps/flow_2.vtu"), ("0", "steps/flow_0.vtu"), ("0.5", "steps/flow_1.vtu")])
)
moved = list(POINTS)
moved[8] = (0.5, 0.5, 2.5)
(root / "steps/moved_1.vtu").write_text(vtu(1, points=moved))
(root / "moving.pvd").write_text(
    collection([("0", "steps/flow_0.vtu"), ("1", "steps/moved_1.vtu")])
)
