#define CALL WRAP
#define WRAP(x) consume(x)
#define ARG 1
int consume(int value);
int sample(void) { CALL(ARG);
#undef ARG
#define ARG 2
return ARG; }
