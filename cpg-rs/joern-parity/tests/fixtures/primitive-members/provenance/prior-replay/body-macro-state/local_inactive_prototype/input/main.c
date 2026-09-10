int choose(int x) {
#define ENABLE 0
#if ENABLE
 int target(int);
#else
 long target(int);
#endif
 return target(x);
}
