typedef int T;
typedef long U;
#define CAST(x) ((T)(U)(x))
int value(int x){return CAST(x);}
