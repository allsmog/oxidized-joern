#define SET(p,v) { int * const slot=(p); *slot=(v); }
void f(int *p, int v) { SET(p,v); }
