int choose(int x) {
#if 0
#include "inactive.h"
 int included;
#else
 int included = x;
#endif
#ifdef LEAK
 return leaked;
#else
 return included;
#endif
}
#if 0
int ignored(void) {
#include "inactive.h"
 return leaked;
}
#endif
int later(int x) {
#ifdef LEAK
 return leaked;
#else
 return x;
#endif
}
