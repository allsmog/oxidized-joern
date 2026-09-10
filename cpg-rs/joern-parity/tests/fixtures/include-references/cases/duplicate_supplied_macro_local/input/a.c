#include "api.h"
local int helper(int value) { return value; }
int entry_a(int value) { return helper(value); }
