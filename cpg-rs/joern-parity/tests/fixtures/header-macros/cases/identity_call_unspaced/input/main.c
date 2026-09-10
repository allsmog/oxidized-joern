#include "defs.h"
int fn(int a, int b);
int probe(int a, int b) { return M(fn(a,b)); }
