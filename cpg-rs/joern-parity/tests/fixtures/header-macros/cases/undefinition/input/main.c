#include "defs.h"
int before(int value) { return PICK(value); }
#undef PICK
int after(int value) { return PICK(value); }
