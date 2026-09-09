int choose(int x) {
#define ENABLE 1
#if ENABLE
 int target(int);
#else
 long target(int);
#endif
 return target(x);
}
