int helper(int);
int out();
int use(int (*)(int));
#define F "%" UNDEF "d"
#define helper(x) (x)
int f(void){int value=out(helper,F);return helper(value);}
