#define PICK 1
#if PICK == 1
#undef PICK
#define PICK 2
#endif
int choose(void) { return PICK; }
