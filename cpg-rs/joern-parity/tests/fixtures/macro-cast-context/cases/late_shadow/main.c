typedef int T;
#define CAST(x) ((T)(x))
int callback(int x);
int value(int x) { int before=CAST(x); int (*T)(int)=callback; return before+CAST(x); }
