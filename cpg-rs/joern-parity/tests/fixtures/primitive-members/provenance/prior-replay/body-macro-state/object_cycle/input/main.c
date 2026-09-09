int CALL(unsigned long value);
#define CALL NEXT
#define NEXT CALL
#define ARG 1
int sample(void) { CALL(sizeof(int[ARG]));
#undef ARG
#define ARG 2
return ARG; }
