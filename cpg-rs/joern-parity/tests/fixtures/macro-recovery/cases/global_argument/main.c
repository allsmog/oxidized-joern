int helper(int);
int out(int, const char *, int);
#define M(x) helper(x)
#define F "%" UNDEF "d"
int global;
int test(int x) { int y = out(global, F, M(x)); return global + y; }
