typedef int (*U)(int);
#define CAST(t,x) ((t)(x))
int check(int x) {return CAST(U,x);}
