# 确定性扰动全部点（伪随机），制造非正交与偏斜
import re,sys
p=sys.argv[1]; amp=float(sys.argv[2])
lines=open(p).read().split("\n")
out=[];k=0
for line in lines:
    m=re.fullmatch(r"\(([-0-9.e]+) ([-0-9.e]+) ([-0-9.e]+)\)",line.strip())
    if m:
        x,y,z=map(float,m.groups()); k+=1
        h=lambda i:(((k*2654435761+i*40503)%1000)/1000.0-0.5)
        x+=amp*h(1);y+=amp*h(2);z+=amp*0.6*h(3)
        line="(%.9g %.9g %.9g)"%(x,y,z)
    out.append(line)
open(p,"w").write("\n".join(out))
