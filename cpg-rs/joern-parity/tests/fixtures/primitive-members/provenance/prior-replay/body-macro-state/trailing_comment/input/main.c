#define CALL WRAP /* trailing comment */
#define WRAP(x) consume(x)
#define ARG 1
int consume(unsigned long value);
int sample(void) { CALL(sizeof(int[ARG]));
#undef ARG
#define ARG 2
return ARG; }
