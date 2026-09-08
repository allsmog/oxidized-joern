#include "api.h"
#if ENABLE
long enabled(int x);
#else
int disabled(int x);
#endif
int g(int x) { return enabled(x) + disabled(x); }
