int helper(int);
int out(int, const char *, int);
#define M(x) helper(x)
#define F "%" UNDEF "d"
int test(int x) { if(out(x,F,M(x))) return M(x); return x; }
