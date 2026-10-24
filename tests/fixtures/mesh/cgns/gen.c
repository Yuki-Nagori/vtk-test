/* 生成 CGNS 测试文件（task 032 PR 3）：libcgns 3.4（HDF5 后端）。用法见 make.sh。 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <cgnslib.h>
#define CK(x) if((x)){fprintf(stderr,"%s:%d %s\n",__FILE__,__LINE__,cg_get_error());exit(1);}

static void coords(int fn,int B,int Z,int np,const double*x,const double*y,const double*z){
  int C;
  CK(cg_coord_write(fn,B,Z,CGNS_ENUMV(RealDouble),"CoordinateX",x,&C));
  CK(cg_coord_write(fn,B,Z,CGNS_ENUMV(RealDouble),"CoordinateY",y,&C));
  CK(cg_coord_write(fn,B,Z,CGNS_ENUMV(RealDouble),"CoordinateZ",z,&C));
  (void)np;
}

/* mixed.cgns：两个 n^3 块。blockHex 为 HEXA_8；blockMixed 每个六面体拆成两个 PENTA_6，
 * 存为带 ElementStartOffset 的 MIXED。两区底面各有 QUAD_4 截面与 FaceCenter BC；
 * Vertex 解含 Pressure 与 VelocityX/Y/Z，CellCenter 解含 CellIndex。 */
static void mixed(const char*path,int n){
  int fn,B,Z,S,F,BC,Fs;
  CK(cg_open(path,CG_MODE_WRITE,&fn));
  CK(cg_base_write(fn,"Base",3,3,&B));
  int np=(n+1)*(n+1)*(n+1), nc=n*n*n;
  double *x=malloc(np*8),*y=malloc(np*8),*z=malloc(np*8),*xs=malloc(np*8);
  #define ID(i,j,k) ((i)+(j)*(n+1)+(k)*(n+1)*(n+1)+1)
  for(int k=0;k<=n;k++)for(int j=0;j<=n;j++)for(int i=0;i<=n;i++){int m=ID(i,j,k)-1;x[m]=i;y[m]=j;z[m]=k;}
  for(int zone=0;zone<2;zone++){
    cgsize_t size[3]={np, zone==0?nc:nc*2, 0};
    CK(cg_zone_write(fn,B,zone==0?"blockHex":"blockMixed",size,CGNS_ENUMV(Unstructured),&Z));
    for(int m=0;m<np;m++) xs[m]=x[m]+(zone?n:0);
    coords(fn,B,Z,np,xs,y,z);
    cgsize_t ncell=0,m=0; cgsize_t *off=malloc((nc*2+1)*sizeof(cgsize_t)); off[0]=0;
    cgsize_t *conn=malloc(nc*2*8*sizeof(cgsize_t));
    for(int k=0;k<n;k++)for(int j=0;j<n;j++)for(int i=0;i<n;i++){
      cgsize_t h[8]={ID(i,j,k),ID(i+1,j,k),ID(i+1,j+1,k),ID(i,j+1,k),ID(i,j,k+1),ID(i+1,j,k+1),ID(i+1,j+1,k+1),ID(i,j+1,k+1)};
      if(zone==0){for(int q=0;q<8;q++)conn[m++]=h[q];ncell++;}
      else{int p1[6]={0,1,2,4,5,6},p2[6]={0,2,3,4,6,7};
        conn[m++]=CGNS_ENUMV(PENTA_6);for(int q=0;q<6;q++)conn[m++]=h[p1[q]];off[ncell+1]=m;
        conn[m++]=CGNS_ENUMV(PENTA_6);for(int q=0;q<6;q++)conn[m++]=h[p2[q]];off[ncell+2]=m;
        ncell+=2;}
    }
    if(zone==0){CK(cg_section_write(fn,B,Z,"Hexes",CGNS_ENUMV(HEXA_8),1,ncell,0,conn,&S))}
    else{CK(cg_poly_section_write(fn,B,Z,"Mixed",CGNS_ENUMV(MIXED),1,ncell,0,conn,off,&S))}
    cgsize_t *q=malloc(n*n*4*sizeof(cgsize_t)),nq=0;
    for(int j=0;j<n;j++)for(int i=0;i<n;i++){q[nq*4]=ID(i,j,0);q[nq*4+1]=ID(i,j+1,0);q[nq*4+2]=ID(i+1,j+1,0);q[nq*4+3]=ID(i+1,j,0);nq++;}
    CK(cg_section_write(fn,B,Z,"Bottom",CGNS_ENUMV(QUAD_4),ncell+1,ncell+nq,0,q,&S));
    cgsize_t range[2]={ncell+1,ncell+nq};
    CK(cg_boco_write(fn,B,Z,"bottomWall",CGNS_ENUMV(BCWall),CGNS_ENUMV(PointRange),2,range,&BC));
    CK(cg_boco_gridlocation_write(fn,B,Z,BC,CGNS_ENUMV(FaceCenter)));
    CK(cg_sol_write(fn,B,Z,"NodeSol",CGNS_ENUMV(Vertex),&Fs));
    double *v=malloc(np*8),*u=malloc(np*8),*w=malloc(np*8);
    for(int a=0;a<np;a++){v[a]=xs[a]+10*y[a];u[a]=1;w[a]=z[a];}
    CK(cg_field_write(fn,B,Z,Fs,CGNS_ENUMV(RealDouble),"Pressure",v,&F));
    CK(cg_field_write(fn,B,Z,Fs,CGNS_ENUMV(RealDouble),"VelocityX",u,&F));
    CK(cg_field_write(fn,B,Z,Fs,CGNS_ENUMV(RealDouble),"VelocityY",w,&F));
    CK(cg_field_write(fn,B,Z,Fs,CGNS_ENUMV(RealDouble),"VelocityZ",u,&F));
    CK(cg_sol_write(fn,B,Z,"CellSol",CGNS_ENUMV(CellCenter),&Fs));
    float *c=malloc(ncell*4); for(int a=0;a<ncell;a++) c[a]=a;
    CK(cg_field_write(fn,B,Z,Fs,CGNS_ENUMV(RealSingle),"CellIndex",c,&F));
    free(off);free(conn);free(q);free(v);free(u);free(w);free(c);
  }
  CK(cg_close(fn));
}

/* tetpyr.cgns：单位立方体按顶点 6 拆成 3 个 PYRA_5，另在 x∈[2,3] 放一个 TETRA_4；
 * 立方体底面为 QUAD_4 截面（无 BC，组名取截面名），x=0 侧的节点集写成 Vertex BC，
 * 顶面 QUAD_4 截面由 Vertex BC "top" 的节点集覆盖。 */
static void tetpyr(const char*path){
  int fn,B,Z,S,BC,Fs,F;
  CK(cg_open(path,CG_MODE_WRITE,&fn));
  CK(cg_base_write(fn,"Base",3,3,&B));
  double x[12]={0,1,1,0,0,1,1,0, 2,3,2,2}, y[12]={0,0,1,1,0,0,1,1, 0,0,1,0}, z[12]={0,0,0,0,1,1,1,1, 0,0,0,1};
  cgsize_t size[3]={12,4,0};
  CK(cg_zone_write(fn,B,"cube",size,CGNS_ENUMV(Unstructured),&Z));
  coords(fn,B,Z,12,x,y,z);
  cgsize_t pyr[15]={1,2,3,4,7, 1,5,6,2,7, 1,4,8,5,7};
  CK(cg_section_write(fn,B,Z,"Pyramids",CGNS_ENUMV(PYRA_5),1,3,0,pyr,&S));
  cgsize_t tet[4]={9,10,11,12};
  CK(cg_section_write(fn,B,Z,"Tet",CGNS_ENUMV(TETRA_4),4,4,0,tet,&S));
  cgsize_t bottom[4]={1,4,3,2}, top[4]={5,6,7,8};
  CK(cg_section_write(fn,B,Z,"cubeBottom",CGNS_ENUMV(QUAD_4),5,5,0,bottom,&S));
  CK(cg_section_write(fn,B,Z,"cubeTop",CGNS_ENUMV(QUAD_4),6,6,0,top,&S));
  cgsize_t topNodes[4]={5,6,7,8};
  CK(cg_boco_write(fn,B,Z,"top",CGNS_ENUMV(BCInflow),CGNS_ENUMV(PointList),4,topNodes,&BC));
  CK(cg_sol_write(fn,B,Z,"Sol",CGNS_ENUMV(CellCenter),&Fs));
  double rank[4]={10,20,30,40};
  CK(cg_field_write(fn,B,Z,Fs,CGNS_ENUMV(RealDouble),"Rank",rank,&F));
  CK(cg_field_write(fn,B,Z,Fs,CGNS_ENUMV(RealDouble),"MomentumX",rank,&F));
  CK(cg_close(fn));
}

/* ngon.cgns：x 方向两个单位立方体，NGON_n 存 11 个面，NFACE_n 存 2 个单元（负号表示该面
 * 法向朝单元内）。x=0 端面以 FaceCenter PointList BC "inlet" 引用。 */
static void ngon(const char*path){
  int fn,B,Z,S,BC;
  CK(cg_open(path,CG_MODE_WRITE,&fn));
  CK(cg_base_write(fn,"Base",3,3,&B));
  double x[12],y[12],z[12];
  for(int k=0;k<2;k++)for(int j=0;j<2;j++)for(int i=0;i<3;i++){int m=i+3*j+6*k;x[m]=i;y[m]=j;z[m]=k;}
  #define N(i,j,k) ((i)+3*(j)+6*(k)+1)
  cgsize_t size[3]={12,2,0};
  CK(cg_zone_write(fn,B,"poly",size,CGNS_ENUMV(Unstructured),&Z));
  coords(fn,B,Z,12,x,y,z);
  /* 面法向：外表面朝外；内部面 x=1 朝 +x（单元 1 外向、单元 2 内向）。 */
  cgsize_t faces[11][4]={
    {N(0,0,0),N(0,0,1),N(0,1,1),N(0,1,0)},  /* 1: x=0, -x */
    {N(1,0,0),N(1,1,0),N(1,1,1),N(1,0,1)},  /* 2: x=1, +x 内部 */
    {N(2,0,0),N(2,1,0),N(2,1,1),N(2,0,1)},  /* 3: x=2, +x */
    {N(0,0,0),N(0,1,0),N(1,1,0),N(1,0,0)},  /* 4: z=0 单元1, -z */
    {N(1,0,0),N(1,1,0),N(2,1,0),N(2,0,0)},  /* 5: z=0 单元2 */
    {N(0,0,1),N(1,0,1),N(1,1,1),N(0,1,1)},  /* 6: z=1 单元1, +z */
    {N(1,0,1),N(2,0,1),N(2,1,1),N(1,1,1)},  /* 7 */
    {N(0,0,0),N(1,0,0),N(1,0,1),N(0,0,1)},  /* 8: y=0 单元1, -y */
    {N(1,0,0),N(2,0,0),N(2,0,1),N(1,0,1)},  /* 9 */
    {N(0,1,0),N(0,1,1),N(1,1,1),N(1,1,0)},  /* 10: y=1 单元1, +y */
    {N(1,1,0),N(1,1,1),N(2,1,1),N(2,1,0)},  /* 11 */
  };
  /* 面 5、9 故意写反，单元 2 以负号引用以检查符号处理。 */
  for(int s=0;s<2;s++){cgsize_t*f=faces[s?8:4];cgsize_t t=f[1];f[1]=f[3];f[3]=t;}
  cgsize_t conn[44],off[12]; off[0]=0;
  for(int f=0;f<11;f++){for(int q=0;q<4;q++)conn[f*4+q]=faces[f][q];off[f+1]=(f+1)*4;}
  CK(cg_poly_section_write(fn,B,Z,"Faces",CGNS_ENUMV(NGON_n),1,11,0,conn,off,&S));
  cgsize_t cells[12]={1,2,4,6,8,10, -2,3,-5,7,-9,11}, coff[3]={0,6,12};
  CK(cg_poly_section_write(fn,B,Z,"Cells",CGNS_ENUMV(NFACE_n),12,13,0,cells,coff,&S));
  cgsize_t inlet[1]={1};
  CK(cg_boco_write(fn,B,Z,"inlet",CGNS_ENUMV(BCInflow),CGNS_ENUMV(PointList),1,inlet,&BC));
  CK(cg_boco_gridlocation_write(fn,B,Z,BC,CGNS_ENUMV(FaceCenter)));
  CK(cg_close(fn));
}

/* transient.cgns（task 033）：两个单四面体 Zone "a"、"b"，BaseIterativeData 声明 3 步
 * TimeValues = 0 / 0.5 / 1，各 Zone 的 ZoneIterativeData/FlowSolutionPointers 指向每步的解。
 * Zone a 用 Sol0..Sol2，另有未被引用的 "Initial"；Zone b 第 2 步写 "Null"（该步缺解）。
 * CellCenter 场 T = 100*步 + Zone 序号，Vertex 场 P = 步 + 节点 x。 */
static void transient(const char*path){
  int fn,B,Z,S,Fs,F;
  CK(cg_open(path,CG_MODE_WRITE,&fn));
  CK(cg_base_write(fn,"Base",3,3,&B));
  CK(cg_simulation_type_write(fn,B,CGNS_ENUMV(TimeAccurate)));
  CK(cg_biter_write(fn,B,"TimeIterValues",3));
  CK(cg_goto(fn,B,"BaseIterativeData_t",1,"end"));
  double times[3]={0,0.5,1}; cgsize_t three=3;
  CK(cg_array_write("TimeValues",CGNS_ENUMV(RealDouble),1,&three,times));
  const char*names[2][3]={{"Sol0","Sol1","Sol2"},{"SolA","Null","SolC"}};
  for(int zone=0;zone<2;zone++){
    double x[4]={0,1,0,0}, y[4]={0,0,1,0}, z[4]={0,0,0,1};
    for(int a=0;a<4;a++) x[a]+=2*zone;
    cgsize_t size[3]={4,1,0};
    CK(cg_zone_write(fn,B,zone?"b":"a",size,CGNS_ENUMV(Unstructured),&Z));
    coords(fn,B,Z,4,x,y,z);
    cgsize_t tet[4]={1,2,3,4};
    CK(cg_section_write(fn,B,Z,"Tet",CGNS_ENUMV(TETRA_4),1,1,0,tet,&S));
    if(zone==0){
      CK(cg_sol_write(fn,B,Z,"Initial",CGNS_ENUMV(CellCenter),&Fs));
      double t=-1; CK(cg_field_write(fn,B,Z,Fs,CGNS_ENUMV(RealDouble),"T",&t,&F));
    }
    for(int step=0;step<3;step++){
      if(!strcmp(names[zone][step],"Null")) continue;
      char cell[33],node[33];
      snprintf(cell,sizeof cell,"%s",names[zone][step]);
      snprintf(node,sizeof node,"%sNodes",names[zone][step]);
      CK(cg_sol_write(fn,B,Z,cell,CGNS_ENUMV(CellCenter),&Fs));
      double t=100*step+zone; CK(cg_field_write(fn,B,Z,Fs,CGNS_ENUMV(RealDouble),"T",&t,&F));
      CK(cg_sol_write(fn,B,Z,node,CGNS_ENUMV(Vertex),&Fs));
      double p[4]; for(int a=0;a<4;a++) p[a]=step+x[a];
      CK(cg_field_write(fn,B,Z,Fs,CGNS_ENUMV(RealDouble),"P",p,&F));
    }
    CK(cg_ziter_write(fn,B,Z,"ZoneIterativeData"));
    CK(cg_goto(fn,B,"Zone_t",Z,"ZoneIterativeData_t",1,"end"));
    char cells[3*32],nodes[3*32]; memset(cells,' ',sizeof cells); memset(nodes,' ',sizeof nodes);
    for(int step=0;step<3;step++){
      char buf[40];
      memcpy(cells+32*step,names[zone][step],strlen(names[zone][step]));
      if(strcmp(names[zone][step],"Null")) snprintf(buf,sizeof buf,"%sNodes",names[zone][step]);
      else snprintf(buf,sizeof buf,"Null");
      memcpy(nodes+32*step,buf,strlen(buf));
    }
    cgsize_t dims[2]={32,3};
    CK(cg_array_write("FlowSolutionPointers",CGNS_ENUMV(Character),2,dims,cells));
    CK(cg_array_write("FlowSolutionVertexPointers",CGNS_ENUMV(Character),2,dims,nodes));
  }
  CK(cg_close(fn));
}

int main(int argc,char**argv){
  if(argc<3){fprintf(stderr,"usage: gen mixed|tetpyr|ngon|transient out.cgns [n]\n");return 2;}
  if(!strcmp(argv[1],"mixed")) mixed(argv[2],argc>3?atoi(argv[3]):2);
  else if(!strcmp(argv[1],"tetpyr")) tetpyr(argv[2]);
  else if(!strcmp(argv[1],"ngon")) ngon(argv[2]);
  else if(!strcmp(argv[1],"transient")) transient(argv[2]);
  else return 2;
  return 0;
}
