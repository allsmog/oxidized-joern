typedef int *P;
#define CAST(x) ((P)(x))
int *value(void *x) { return CAST(x); }
