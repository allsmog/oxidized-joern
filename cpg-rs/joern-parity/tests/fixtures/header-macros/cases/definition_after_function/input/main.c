#include "defs.h"
int before(int value) { return PICK(value); }
#define PICK(value) ((value) + 1)
int after(int value) { return PICK(value); }
