#define INNER 7
#define ARG 3
#define WRAP(x) ((x)+INNER)
int choose(void) { return WRAP(ARG); }
