#include "defs.h"
int inner(int value);
int outer(int value);
int probe(int value) { return WRAP(value); }
