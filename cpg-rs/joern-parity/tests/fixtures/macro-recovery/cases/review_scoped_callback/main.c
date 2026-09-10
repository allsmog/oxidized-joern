int helper(int);
int out(const char *, int (*)(int));
#define F "%" UNDEF "d"
int f(int (*helper)(int)) { int value=out(F, helper); return value; }
