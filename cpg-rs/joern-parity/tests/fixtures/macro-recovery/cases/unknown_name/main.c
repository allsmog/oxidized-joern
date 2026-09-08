int helper(int);
int out(int, const char *, int);
#define M(x) helper(x)
int test(int x) { int y = out(x, UNDEF, M(x)); return y; }
