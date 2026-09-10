#define SET(p,v) for(;v;) { *(p)=v; break; }
void f(int *p, int v) { SET(p,v); }
