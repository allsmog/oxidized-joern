#undef __STDC_VERSION__
#if defined(__STDC_VERSION__)
int selected_true(int value) { return value + 1; }
#else
int selected_false(int value) { return value + 2; }
#endif
