#!/bin/bash
# 生成 OpenFOAM 测试算例（OpenFOAM v1912，Debian openfoam 1912.200626）
set -e
source /usr/share/openfoam/etc/bashrc >/dev/null 2>&1 || true
HDR='FoamFile { version 2.0; format ascii; class dictionary; object X; }'
mkcase() { # dir nx ny nz
  local d=$1; rm -rf $d; mkdir -p $d/system $d/constant $d/0
  cp ../cavity/system/fvSchemes ../cavity/system/fvSolution $d/system/
  cp ../cavity/constant/transportProperties $d/constant/
  cp ../cavity/0/U ../cavity/0/p $d/0/
  cat > $d/system/controlDict <<C
${HDR/X/controlDict}
application icoFoam; startFrom startTime; startTime 0; stopAt endTime;
endTime 0.01; deltaT 0.005; writeControl timeStep; writeInterval 1; purgeWrite 0;
writeFormat ascii; writePrecision 8; writeCompression off; timeFormat general; timePrecision 6; runTimeModifiable false;
C
  cat > $d/system/blockMeshDict <<B
${HDR/X/blockMeshDict}
scale 0.1;
vertices ((0 0 0) (1 0 0) (1 1 0) (0 1 0) (0 0 0.1) (1 0 0.1) (1 1 0.1) (0 1 0.1));
blocks (hex (0 1 2 3 4 5 6 7) ($2 $3 $4) simpleGrading (1 1 1));
edges ();
boundary (
  movingWall { type wall; faces ((3 7 6 2)); }
  fixedWalls { type wall; faces ((0 4 7 3) (2 6 5 1) (1 5 4 0)); }
  frontAndBack { type wall; faces ((0 3 2 1) (4 5 6 7)); }
);
mergePatchPairs ();
B
  # frontAndBack 改为 wall，以便 3D 层数 > 1 时 icoFoam 正常
  sed -i 's/empty/zeroGradient/; s/type *empty;/type zeroGradient;/' $d/0/p
  python3 - $d <<'P'
import re,sys
d=sys.argv[1]
for f,val in (("U","type noSlip;"),("p","type zeroGradient;")):
    s=open(f"{d}/0/{f}").read()
    s=re.sub(r"frontAndBack\s*\{[^}]*\}", "frontAndBack { %s }"%val, s)
    open(f"{d}/0/{f}","w").write(s)
P
  touch $d/$(basename $d).foam
}
mkcase ascii 3 2 2
(cd ascii && blockMesh >log.bm && icoFoam >log.ico)
# 二进制 + gzip
rm -rf binary && cp -r ascii binary && mv binary/ascii.foam binary/binary.foam
(cd binary && foamDictionary -entry writeFormat -set binary system/controlDict >/dev/null && foamDictionary -entry writeCompression -set on system/controlDict >/dev/null && foamFormatConvert >log.conv)
# 多面体：polyDualMesh
mkcase poly 3 3 2
(cd poly && blockMesh >log.bm && polyDualMesh 80 -overwrite >log.dual && icoFoam >log.ico)
# 非正交 / 偏斜（032 PR 2）：斜六面体块 + 渐变 + 确定性扰动，及其对偶网格
mkcase skew 4 3 2
sed -i 's/^vertices .*/vertices ((0 0 0) (1 0 0) (1.2 1 0) (0.3 1 0) (0.1 0 0.4) (1 0.1 0.5) (1.3 1.1 0.6) (0.4 1 0.4));/; s/(4 3 2) simpleGrading (1 1 1)/(4 3 2) simpleGrading (3 1 0.5)/' skew/system/blockMeshDict
(cd skew && blockMesh >log.bm && python3 ../perturb.py constant/polyMesh/points 0.006 && checkMesh >log.check)
rm -rf skewdual && cp -r skew skewdual && mv skewdual/skew.foam skewdual/skewdual.foam
(cd skewdual && polyDualMesh 80 -overwrite >log.dual && checkMesh >log.check)
# 多时间步（033 PR 1）：沿用 ascii 网格，endTime 0.015，保留 0 / 0.005 / 0.01 / 0.015 四个时间目录
rm -rf transient && cp -r ascii transient && mv transient/ascii.foam transient/transient.foam
(cd transient && foamDictionary -entry endTime -set 0.015 system/controlDict >/dev/null && icoFoam >log.ico)
rm -rf transient/*/uniform transient/*/phi
