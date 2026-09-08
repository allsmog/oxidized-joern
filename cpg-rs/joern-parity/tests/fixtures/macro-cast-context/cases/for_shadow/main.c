typedef int T;
#define CAST(x) ((T)(x))
int callback(int x);
int use(int x);
int value(int x){for(int (*T)(int)=callback;x;x=0){use(CAST(x));}return CAST(x);}
