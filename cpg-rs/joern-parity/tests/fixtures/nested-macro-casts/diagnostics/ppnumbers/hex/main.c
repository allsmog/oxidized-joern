#define M(x) ((x)+7)
#define OUT(x) 0xM(x)
int f(int x) { return OUT(x); }
