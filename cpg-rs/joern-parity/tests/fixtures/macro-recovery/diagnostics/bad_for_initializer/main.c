int helper(int);
int out(int, const char *, int);
#define M(x) helper(x)
#define F "%" UNDEF "d"
int test(int x) { for(int y=out(x,F,M(x));y;x--) { x=y; } return M(x); }
