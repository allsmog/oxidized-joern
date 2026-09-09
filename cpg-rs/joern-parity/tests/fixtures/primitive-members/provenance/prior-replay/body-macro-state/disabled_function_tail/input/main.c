int F(unsigned long value);
#define CALL F(0)
#define F(x) F
#define ARG 1
int sample(void) { CALL(sizeof(int[ARG]));
#undef ARG
#define ARG 2
return ARG; }
