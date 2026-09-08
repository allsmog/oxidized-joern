#include "api.h"
#ifdef FLAG
int hidden(int x) {
#if 1
 return x;
#endif
}
#endif
int visible(int x) { return x; }
