#define SET(p,v) for(int i=0;i<v;i++) { *(p)=i; }
void f(int *p, int v) { SET(p,v); }
