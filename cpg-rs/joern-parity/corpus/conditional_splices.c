#define FLAG 1
#undef FLAG /* removed */
#ifdef FLAG
int undef_comment_wrong(int x) { return x; }
#else
int undef_comment_active(int x) { return x; }
#endif
#define MULTI 1 \
 + 2
#if MULTI == 3
int spliced_macro_active(int x) { return x; }
#else
int spliced_macro_wrong(int x) { return x; }
#endif
#if 1 \
 && 1
int spliced_condition_active(int x) { return x; }
#else
int spliced_condition_wrong(int x) { return x; }
#endif
