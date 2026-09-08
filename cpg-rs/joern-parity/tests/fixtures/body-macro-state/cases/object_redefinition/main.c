#define CALL consume
#define ARG 1
int consume(int value);
int sample(void) { CALL(ARG);
#undef ARG
#define ARG 2
return ARG; }
