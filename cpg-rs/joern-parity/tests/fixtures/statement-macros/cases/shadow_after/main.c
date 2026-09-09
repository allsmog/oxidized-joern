#define SET(p,v) { int temp=(v); *(p)=temp; }
int f(int *p, int v) { int temp=7; SET(p,v); return temp; }
