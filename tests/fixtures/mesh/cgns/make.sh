#!/bin/bash
# 生成 CGNS 测试文件（Debian libcgns 3.4 + libhdf5 1.10 serial，HDF5 后端）。
set -e
cd "$(dirname "$0")"
work=$(mktemp -d)
cc -O1 -o "$work/gen" gen.c -lcgns
cc -O1 -o "$work/strip" strip_offsets.c -l:libhdf5_serial.so.310
"$work/gen" mixed mixed.cgns 2
cp mixed.cgns mixed_v3.cgns
"$work/strip" mixed_v3.cgns /Base/blockMixed/Mixed/ElementStartOffset
"$work/gen" tetpyr tetpyr.cgns
"$work/gen" ngon ngon.cgns
"$work/gen" transient transient.cgns
# ADF 后端样本：只用于验证拒绝路径。
cgnsconvert -a tetpyr.cgns adf.cgns >/dev/null
for f in mixed tetpyr ngon transient; do cgnscheck "$f.cgns" >"$work/$f.check" 2>&1 || { cat "$work/$f.check"; exit 1; }; done
rm -rf "$work"
