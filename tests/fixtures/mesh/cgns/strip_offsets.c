/* 删除 MIXED 截面的 ElementStartOffset，得到 CGNS 3.3 及更早版本的无偏移布局。
 * 系统未装 HDF5 头文件，这里只声明用到的 3 个 C API（hid_t 为 int64_t）。 */
#include <stdint.h>
#include <stdio.h>
typedef int64_t hid_t;
hid_t H5Fopen(const char*, unsigned, hid_t);
int H5Ldelete(hid_t, const char*, hid_t);
int H5Fclose(hid_t);
int main(int argc,char**argv){
  if(argc<3) return 2;
  hid_t f=H5Fopen(argv[1],1u,0); if(f<0) return 1;
  for(int i=2;i<argc;i++) if(H5Ldelete(f,argv[i],0)<0){fprintf(stderr,"delete %s failed\n",argv[i]);return 1;}
  return H5Fclose(f)<0;
}
