#include "defs.h"
int probe(int p) { return p; }
int invoke(int p) { checkstackp(0, 1, p); return p; }
