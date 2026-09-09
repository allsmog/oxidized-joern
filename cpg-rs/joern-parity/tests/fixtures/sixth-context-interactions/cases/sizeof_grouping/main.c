typedef int T;
#define SIZE (sizeof(T)-1)
#define COUNT(x) (sizeof(x)/sizeof(T)-1)
int value(int x){return SIZE+COUNT(x);}
