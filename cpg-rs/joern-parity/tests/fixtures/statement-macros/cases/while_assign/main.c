#define SET(p,v) while (v) { *(p) = (v); --v; }
void f(int *p, int v) { SET(p,v); }
