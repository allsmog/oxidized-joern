#include "api.h"
#define CAST(x) ((T)(x))
#define SIZE sizeof(T)
RET value(T x){return CAST(x)+SIZE+declared(x);}
