#define PP_PRESENT 1
#if defined(PP_PRESENT)
typedef int pp_word;
int pp_global = 2;
struct PpBox { int value; };
int pp_defined(int x);
int pp_if(int x) { return x; }
#if 0
typedef int pp_inactive_word;
int pp_inactive_global = 9;
struct PpInactiveBox { int value; };
int pp_defined(int x);
int pp_nested_false(int x) { return x + 1; }
#elif 1
int pp_nested_elif(int x) { return x + 2; }
#else
int pp_nested_else(int x) { return x + 3; }
#endif
#elif 0
int pp_elif(int x) { return x + 4; }
#else
int pp_else(int x) { return x + 5; }
#endif
#ifndef PP_ABSENT
int pp_ifndef(int x) { return x + 6; }
#endif
#if 0
int pp_false(int x) { return x + 7; }
#else
int pp_default(int x) { return x + 8; }
#endif
int pp_defined(int x) { return x + pp_global; }
