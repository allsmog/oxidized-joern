#include "api.h"
int choose(int x) {
#if ENABLE
 int target(int);
#else
 long target(int);
#endif
 return target(x);
}
