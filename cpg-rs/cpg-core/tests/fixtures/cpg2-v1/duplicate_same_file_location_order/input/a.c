#if 1
static int helper(int value) { return value + 1; }
#else
static int helper(int value) { return value + 2; }
#endif
int entry_a(int value) { return helper(value); }
