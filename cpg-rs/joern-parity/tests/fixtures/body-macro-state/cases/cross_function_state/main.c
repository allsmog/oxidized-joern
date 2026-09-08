void setup(void) {
#define AFTER 1
}
int choose(int x) {
#if AFTER
 return x;
#else
 return 0;
#endif
}
