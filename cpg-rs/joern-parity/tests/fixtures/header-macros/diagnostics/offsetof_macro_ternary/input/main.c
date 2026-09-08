#include <stddef.h>
typedef struct { int x; } A;
#define OFF(a) ((a)?offsetof(A,x):0)
int probe(int a) { return OFF(a); }
