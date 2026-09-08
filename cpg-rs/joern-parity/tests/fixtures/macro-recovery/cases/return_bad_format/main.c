int helper(int);
int out(int, const char *, int);
#define M(x) helper(x)
#define F "%" UNDEF "d"
int test(int x) { return out(x, F, M(x)); }
