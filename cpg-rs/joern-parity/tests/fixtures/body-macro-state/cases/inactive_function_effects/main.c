#if 0
void skipped(void) {
#define FLAG 1
}
#endif
int choose(int x) {
#ifdef FLAG
 return x;
#else
 return 0;
#endif
}
