#define FLAG 1
int choose(int x) {
#if 0
#undef FLAG
#define HIDDEN 1
#endif
#if defined(FLAG) && !defined(HIDDEN)
 return x;
#else
 return 0;
#endif
}
