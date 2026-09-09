#include "switch.h"
int choose(int x) {
#undef FLAG
#ifdef FLAG
 return x;
#else
 return 0;
#endif
}
