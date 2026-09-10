int helper(int);
int out(int, const char *, int);
#define M(x) helper(x)
#define F "%" UNDEF "d"
int test(int x) { int y = x ? out(x, F, M(x)) : M(x); return y; }
