#define CALL consume
#define ARG 1
int consume(int value);
int sample(void) { return CALL(ARG); }
