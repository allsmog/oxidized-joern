typedef int T;
#define CAST(x) ((T)(x))
int value(int (*T)(int),int x) { return CAST(x); }
