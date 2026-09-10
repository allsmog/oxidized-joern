static int helper(int value);
int helper(int value) { return value + 1; }
int entry_b(int value) { return helper(value); }
