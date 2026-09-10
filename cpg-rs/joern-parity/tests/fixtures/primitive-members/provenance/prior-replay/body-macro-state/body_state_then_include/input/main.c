void setup(void) {
#define SELECT 1
}
#include "api.h"
int choose(int x) {
#if AFTER
 return x;
#else
 return 0;
#endif
}
