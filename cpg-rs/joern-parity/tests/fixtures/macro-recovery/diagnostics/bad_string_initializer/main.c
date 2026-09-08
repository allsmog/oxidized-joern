int helper(int);
int out(int, const char *, int);
#define M(x) helper(x)
#define F "%" UNDEF "d"
int test(int x) { const char *y=F; return out(x,y,M(x)); }
