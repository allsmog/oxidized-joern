#define LEVEL 2
#define LIMIT (LEVEL + 1)
#if defined(LEVEL) && LIMIT == 3
#define STEP(x) ((x) + 1)
int selected_expression(int x) { return STEP(x); }
#else
#define STEP(x) ((x) + 2)
int unselected_expression(int x) { return STEP(x); }
#endif
#undef LEVEL
#ifdef LEVEL
int after_undef_bad(int x) { return x + 2; }
#else
int after_undef_good(int x) { return x + 3; }
#endif
#ifdef FUTURE
int before_define_bad(int x) { return x + 4; }
#else
int before_define_good(int x) { return x + 5; }
#endif
#define FUTURE 1
#if 0
#define LEAK 1
#endif
#ifdef LEAK
int inactive_define_bad(int x) { return x + 6; }
#else
int inactive_define_good(int x) { return x + 7; }
#endif
