#if defined(__STDC__) && !defined(__cplusplus)
int selected_true(int value) { return value + 1; }
#else
int selected_false(int value) { return value + 2; }
#endif
