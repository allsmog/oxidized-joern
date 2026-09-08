typedef int T;
#define CAST(x) ((T)(x))
#if 0
typedef long U;
#endif
int use(int value) { return CAST(value) + (U)(value); }
