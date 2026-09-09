#define ARG 1
int consume(int value);
int sample(void) { consume(ARG);
#undef ARG
#define ARG 2
return ARG; }
