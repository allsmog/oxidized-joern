#define BASE unsigned UNKNOWN
#define CAST(t,x) ((t)(x))
int check(int x) {typedef BASE U;return CAST(U,x);}
