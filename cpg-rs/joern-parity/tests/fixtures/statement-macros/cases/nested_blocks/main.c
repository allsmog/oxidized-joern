#define SET(p,v) { { int x=(v); *(p)=x; } *(p)+=1; }
void f(int *p, int v) { SET(p,v); }
