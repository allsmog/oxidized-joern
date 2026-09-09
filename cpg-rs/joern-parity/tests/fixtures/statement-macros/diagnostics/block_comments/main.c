#define SET(p,v) { /* slot */ int *slot=(p); /* store */ *slot=(v); }
void f(int *p, int v) { SET(p,v); }
