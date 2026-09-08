#define __STDC__ 0
#if __STDC__
int selected_true(int value) { return value + 1; }
#else
int selected_false(int value) { return value + 2; }
#endif
