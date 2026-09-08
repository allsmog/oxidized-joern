int helper(int);
int out(int, const char *, int);
#define M(x) helper(x)
#define F "%" UNDEF "d"
int test(int x) { int a=M(x); int y=out(a,F,M(x)); return a+y; }
