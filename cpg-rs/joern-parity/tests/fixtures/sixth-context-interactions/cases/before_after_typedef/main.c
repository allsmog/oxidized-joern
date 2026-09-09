#define C(x) ((T)(x))
int f(int x) { int a = C(x); typedef int T; return a+C(x); }
