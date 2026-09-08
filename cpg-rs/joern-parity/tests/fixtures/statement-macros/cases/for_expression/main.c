#define SET(p,v) for(v=0;v<2;v++) { *(p)=v; }
void f(int *p, int v) { SET(p,v); }
