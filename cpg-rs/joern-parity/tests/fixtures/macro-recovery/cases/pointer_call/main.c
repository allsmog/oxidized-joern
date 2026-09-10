int helper(int);
int out(int, const char *, int);
#define M(x) helper(x)
#define F "%" UNDEF "d"
int test(int x, int (*fp)(int,const char*,int)) { int y=fp(x,F,M(x)); return y; }
