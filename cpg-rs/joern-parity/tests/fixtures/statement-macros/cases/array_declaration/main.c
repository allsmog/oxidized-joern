#define SET(v) { int a[2]={(v),1}; use(a); }
void use(int *a);
void f(int v) { SET(v); }
