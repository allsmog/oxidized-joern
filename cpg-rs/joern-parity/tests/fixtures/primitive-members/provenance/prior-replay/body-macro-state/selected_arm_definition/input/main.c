int choose(int x) {
#define FLAG 1
#if FLAG
#define CHOICE 1
#else
#define CHOICE 0
#endif
#if CHOICE
 return x;
#else
 return 0;
#endif
}
