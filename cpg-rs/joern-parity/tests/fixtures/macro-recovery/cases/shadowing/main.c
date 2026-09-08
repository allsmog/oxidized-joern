int helper(int);
int out(int, const char *, int);
#define M(x) helper(x)
#define F "%" UNDEF "d"
int test(int x) { int y=out(x,F,M(x)); { int y=M(x); out(y,"%d",M(x)); } return y; }
