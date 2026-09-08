#define FLAG 1
#  undef OTHER
#ifdef FLAG
int kept(int x) {
#if 1
 return x;
#else
 return 0;
#endif
}
#endif
