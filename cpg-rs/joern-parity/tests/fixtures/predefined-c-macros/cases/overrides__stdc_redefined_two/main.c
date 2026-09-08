#undef __STDC__
#define __STDC__ 2
#if __STDC__ == 2
int selected_true(int value) { return value + 1; }
#else
int selected_false(int value) { return value + 2; }
#endif
