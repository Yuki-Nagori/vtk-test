set -e
source /usr/share/openfoam/etc/bashrc >/dev/null 2>&1 || true
d=multi; rm -rf $d; cp -r ascii $d; rm -rf $d/0.005 $d/0.01 $d/log.* $d/ascii.foam; touch $d/multi.foam
cd $d
sed -i 's/(3 2 2)/(4 2 1)/' system/blockMeshDict
blockMesh > log.bm
cat > system/topoSetDict <<T
FoamFile { version 2.0; format ascii; class dictionary; object topoSetDict; }
actions (
 { name solidCells; type cellSet; action new; source boxToCell; sourceInfo { box (0 0 0) (0.05 0.1 0.01); } }
 { name solid; type cellZoneSet; action new; source setToCellZone; sourceInfo { set solidCells; } }
 { name fluidCells; type cellSet; action new; source cellToCell; sourceInfo { set solidCells; } }
 { name fluidCells; type cellSet; action invert; }
 { name fluid; type cellZoneSet; action new; source setToCellZone; sourceInfo { set fluidCells; } }
);
T
topoSet > log.topo
cat > 0/T <<T
FoamFile { version 2.0; format ascii; class volScalarField; object T; }
dimensions [0 0 0 1 0 0 0];
internalField uniform 300;
boundaryField { ".*" { type zeroGradient; } }
T
rm 0/p
cat > 0/U <<T
FoamFile { version 2.0; format ascii; class volVectorField; object U; }
dimensions [0 1 -1 0 0 0 0];
internalField uniform (1 0 0);
boundaryField { ".*" { type zeroGradient; } }
T
splitMeshRegions -cellZones -overwrite > log.split
ls constant 0 constant/*/polyMesh 0/*
