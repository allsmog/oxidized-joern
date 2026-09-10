#define SET(p,v) { *(p) = (v); *(p) += 1; }
void f(int *p, int v) { SET(p,v); SET(p,2); }
