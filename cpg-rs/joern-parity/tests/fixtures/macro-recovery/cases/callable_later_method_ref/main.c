int helper(int);
int out();
int use(int (*)(int));
#define F "%" UNDEF "d"
int f(void){int value=out(helper,F);use(helper);return value;}
