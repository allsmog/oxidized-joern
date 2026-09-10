#define SET(p,v) do { *(p) = (v); } while (0)
void f(int *p, int v) { SET(p,v); }
