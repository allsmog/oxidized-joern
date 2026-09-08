typedef int T;
#define CAST(t,x) ((t)(x))
#define OUT(x) CAST(T,x)
int value(int x) { return OUT(x); }
