#define CALL NEXT
#define NEXT consume
#define ARG 1
int consume(unsigned long value);
int sample(void) { CALL(sizeof(int[ARG]));
#undef ARG
#define ARG 2
return ARG; }
