#define SET(p,v) { const unsigned char c=(v); int **q=(p); int n; use(c,q,n); }
void use(int a,int **b,int c);
void f(int **p, int v) { SET(p,v); }
