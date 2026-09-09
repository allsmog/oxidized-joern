int ordinary(int value) { return value; }
static int helper(int value) { return value + 1; }
int entry(int value) { return ordinary(value) + helper(value); }
