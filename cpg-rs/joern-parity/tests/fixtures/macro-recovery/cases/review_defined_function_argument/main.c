int helper(int x) { return x; }
int out(const char *, int (*)(int));
#define F "%" UNDEF "d"
int f(void) { int value = out(F, helper); return value; }
