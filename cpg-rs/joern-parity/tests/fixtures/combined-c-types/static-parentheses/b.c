static int (helper)(int value) { return value + 1; }
int use_b(int value) { return helper(value); }
