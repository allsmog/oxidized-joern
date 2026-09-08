#define CAST(x) ((T)(x))
int before(int x){return CAST(x)+declared(x);}
#include "api.h"
RET after(int x){return CAST(x)+sizeof(T)+declared(x);}
