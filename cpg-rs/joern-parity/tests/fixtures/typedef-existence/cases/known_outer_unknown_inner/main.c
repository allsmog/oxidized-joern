typedef int T;
#define CAST(x) ((U)(x))
#define ADD(a,b) ((T)(CAST(a)+CAST(b)))
int direct(int x){return CAST(x);}
int add(int x,int y){return ADD(x,y);}
