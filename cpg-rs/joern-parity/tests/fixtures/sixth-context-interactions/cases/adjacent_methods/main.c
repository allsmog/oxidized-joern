#define C(x) ((T)(x))
int z_with(int x) { typedef int T; return C(x); }
int a_without(int x) { return C(x); }
