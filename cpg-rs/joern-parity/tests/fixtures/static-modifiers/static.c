int plain(int value) { return value; }
static int local_static(int value) { return value; }
extern int external(int value) { return value; }
inline int inline_function(int value) { return value; }
static inline int local_inline(int value) { return value; }
inline static int inline_local(int value) { return value; }
extern inline int external_inline(int value) { return value; }
static int static_prototype(int value);
static inline int static_inline_prototype(int value);
static int inherited_static(int value);
int inherited_static(int value) { return value; }
int later_static(int value);
static int later_static(int value) { return value; }
#if 0
static int inactive_static(int value) { return value; }
#endif
static void zero_params(void) {}
