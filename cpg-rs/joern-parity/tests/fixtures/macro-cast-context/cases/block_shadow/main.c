typedef int T;
#define CAST(x) ((T)(x))
int use(int x);
int callback(int x);
int value(int x) { use(CAST(x)); { int (*T)(int)=callback; use(CAST(x)); } return CAST(x); }
