#  undef FLAG
#define FLAG 1
#ifdef FLAG
int kept(int x) {
#if 1
 return x;
#else
 return 0;
#endif
}
#endif
