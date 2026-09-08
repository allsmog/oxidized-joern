#define CAST(x) ((T)(x))
int value(int x) { typedef int T; return CAST(x); }
