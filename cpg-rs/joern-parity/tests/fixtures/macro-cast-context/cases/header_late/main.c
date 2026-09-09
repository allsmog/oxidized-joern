#define CAST(x) ((T)(x))
int before(int x) { return CAST(x); }
#include "types.h"
int after(int x) { return CAST(x); }
