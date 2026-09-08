#define FORWARD(L,p,v) { int temp=(v); sink(temp); touch(L,p); }
int source(void);
void sink(int v);
void touch(int a,int b);
void f(int L, int p) { int v=source(); FORWARD(L,p,v); }
