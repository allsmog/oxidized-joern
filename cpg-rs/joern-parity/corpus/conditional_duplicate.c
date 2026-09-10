#if 0
int duplicate(int x) { return x + 1; }
#else
int duplicate(int x) { return x + 2; }
#endif
int call_duplicate(int x) { return duplicate(x); }
