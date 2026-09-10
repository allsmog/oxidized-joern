#define WRITE(p,v) (*(p)=(v))
#define SET(L,p,v) { int *slot=(p); WRITE(slot,v); use(L); }
void use(int v);
void f(int L, int *p, int v) { SET(L,p,v); }
