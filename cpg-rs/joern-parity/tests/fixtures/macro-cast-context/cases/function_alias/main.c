typedef int (*F)(int);
#define CAST(x) ((F)(x))
void *value(void *x) { return CAST(x); }
