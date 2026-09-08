#define CALL consume
#define WRAP(x) (x)
#define ARG 1
int consume(int value);
int sample(void) { CALL(WRAP(ARG));
#undef WRAP
#define WRAP(x) ((x)+1)
return WRAP(ARG); }
