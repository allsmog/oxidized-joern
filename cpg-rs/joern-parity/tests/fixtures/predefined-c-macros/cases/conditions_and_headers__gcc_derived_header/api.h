#include "config.h"
#ifndef STDC
void *reserve(unsigned amount);
void release(void *pointer);
int fallback_enabled(int value) { return value + 1; }
#else
int fallback_disabled(int value) { return value + 2; }
#endif
