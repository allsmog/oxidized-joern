#include "api.h"
RET first(int x){return f(x);}
#undef RET
#define RET int
RET second(int x){return f(x);}
