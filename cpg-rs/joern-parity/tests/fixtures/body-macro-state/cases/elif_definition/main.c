int choose(int x) {
#if 0
#define CHOICE 0
#elif 1
#define CHOICE 1
#else
#undef CHOICE
#endif
#if CHOICE
 return x;
#else
 return 0;
#endif
}
