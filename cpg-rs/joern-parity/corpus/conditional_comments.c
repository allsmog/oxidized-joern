#define FLAG 1
#if defined(/*operand*/ FLAG)
int defined_comment_active(int x) { return x; }
#else
int defined_comment_wrong(int x) { return x; }
#endif
#if defined /*name*/ FLAG
int defined_space_active(int x) { return x; }
#else
int defined_space_wrong(int x) { return x; }
#endif
#if 1 /* multiline
comment */ && 0
int multiline_wrong(int x) { return x; }
#else
int multiline_active(int x) { return x; }
#endif
#define MACRO 0 /* inline */ + 1
#if MACRO
int macro_comment_active(int x) { return x; }
#else
int macro_comment_wrong(int x) { return x; }
#endif
