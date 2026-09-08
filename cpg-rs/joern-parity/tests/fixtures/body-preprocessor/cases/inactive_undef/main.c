#define FLAG 1
#if 0
#  undef FLAG
#endif
#ifdef FLAG
int kept(int x) {
#if 1
 return x;
#else
 return 0;
#endif
}
#endif
