#define PICK 1
int choose(void) {
#if PICK == 1
#undef PICK
#define PICK 2
#elif PICK == 2
#define WRONG 1
#else
#undef PICK
#define PICK 9
#endif
#if defined(WRONG)
 return 0;
#else
 return PICK;
#endif
}
int later(void) { return PICK; }
