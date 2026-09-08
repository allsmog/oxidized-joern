typedef int T;
#define CAST(x) ((T)(x))
int value(int a,int b) { return (T)(a,b) + CAST((a,b)); }
