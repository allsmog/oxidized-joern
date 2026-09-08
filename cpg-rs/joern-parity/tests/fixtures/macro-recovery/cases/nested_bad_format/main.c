int helper(int);
int out(int, const char *, int);
#define M(x) helper(x)
#define BAD "%" UNDEF "d"
#define F BAD
int test(int x) { int y = out(x, F, M(x)); return y; }
