#define CAST(x) ((T)(x))
int value(int x){
#if 0
int T;
#else
typedef int T;
#endif
return CAST(x);}
