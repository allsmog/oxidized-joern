#define WRITE(p,v) do { *(p)=(v); } while (0)
#define SET(p,v) do { WRITE(p,v); use(v); } while (0)
void use(int v);
void f(int *p, int v) { SET(p,v); }
