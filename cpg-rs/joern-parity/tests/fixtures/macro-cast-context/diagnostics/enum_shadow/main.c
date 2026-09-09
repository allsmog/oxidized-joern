typedef int T;
#define CAST(x) ((T)(x))
int value(int x){enum { T=1 };return CAST(x);}
