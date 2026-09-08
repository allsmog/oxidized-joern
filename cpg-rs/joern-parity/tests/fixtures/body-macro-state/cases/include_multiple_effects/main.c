void setup(void) {
#define VALUE 1
}
#include "api.h"
int choose(int x) {
#if FLAG && VALUE == 3
 return x;
#else
 return 0;
#endif
}
