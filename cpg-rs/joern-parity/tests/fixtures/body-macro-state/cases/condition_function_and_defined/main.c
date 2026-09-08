#define PICK(x) (x)
#define FLAG 1
#if defined(FLAG) && PICK(FLAG)
#undef PICK
#define PICK(x) ((x)+2)
#endif
int choose(void) { return PICK(3); }
