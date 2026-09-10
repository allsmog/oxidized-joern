#define VALUE 1 + 2
#if VALUE * 3 == 7
int precedence_active(void) { return 7; }
#else
int precedence_wrong(void) { return 9; }
#endif
#if 1 ? 1 : 0
int ternary_active(void) { return 1; }
#else
int ternary_wrong(void) { return 0; }
#endif
#if 'A' == 65
int character_active(void) { return 65; }
#else
int character_wrong(void) { return 0; }
#endif
