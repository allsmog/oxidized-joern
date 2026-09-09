#define DECL typedef int T
#define C(x) ((T)(x))
int f(int x) { DECL; return C(x); }
