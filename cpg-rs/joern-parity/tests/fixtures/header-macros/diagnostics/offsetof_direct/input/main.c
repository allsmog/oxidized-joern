#include <stddef.h>
typedef struct { int x; } A;
int probe(void) { return offsetof(A, x); }
