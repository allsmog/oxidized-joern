#include "defs.h"
int before(int value) { return PICK(value); }
#undef PICK
#define PICK(value) ((value) + 2)
int after(int value) { return PICK(value); }
