#if 1
#include "conditional_active.h"
#else
#include "conditional_inactive.h"
#endif
int conditional_included(int x) { return x; }
