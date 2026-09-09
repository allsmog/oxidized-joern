#define SET(L,p,v) do { int *slot=(p); *slot=(v); use(L); } while(0)
void use(int v);
void f(int L, int *p, int v) { SET(L,p,v); }
