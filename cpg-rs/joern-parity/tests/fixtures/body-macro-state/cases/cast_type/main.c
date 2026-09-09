#define CALL consume
#define ARG int
int consume(int value);
int sample(void) { CALL((ARG)0);
#undef ARG
#define ARG 2
return ARG; }
