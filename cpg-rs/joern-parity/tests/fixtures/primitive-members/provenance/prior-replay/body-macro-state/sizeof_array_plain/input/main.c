#define ARG 1
int consume(unsigned long value);
int sample(void) { consume(sizeof(int[ARG]));
#undef ARG
#define ARG 2
return ARG; }
