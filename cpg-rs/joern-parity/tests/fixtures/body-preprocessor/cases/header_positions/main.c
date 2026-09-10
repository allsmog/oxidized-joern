int before(int x) {
#if FLAG
 return discarded;
#else
 return x;
#endif
}
#include "api.h"
int after(int x) {
#if FLAG
 return x;
#else
 return discarded;
#endif
}
