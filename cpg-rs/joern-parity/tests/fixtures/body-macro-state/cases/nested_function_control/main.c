#define FLAG 1
int choose(int x) {
 int inner(int y) { return y; }
#if FLAG
 return x;
#else
 return 0;
#endif
}
