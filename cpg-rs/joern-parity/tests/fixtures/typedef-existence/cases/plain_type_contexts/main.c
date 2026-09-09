typedef unsigned long long U;
#define CAST(x) ((U)(x))
#define ADD(a,b) ((U)(CAST(a)+CAST(b)))
int direct(int x){return CAST(x);}
int add(int x,int y){return ADD(x,y);}
