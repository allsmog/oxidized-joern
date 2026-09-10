#define CALL consume
#define ARG int
int consume(unsigned long value);
int sample(void) { CALL(sizeof(ARG));
#undef ARG
#define ARG 2
return ARG; }
